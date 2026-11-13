use std::mem;

use anyhow::Result;
use derive_where::derive_where;
use enum_display::EnumDisplay;
use screeps::{BUILD_POWER, ConstructionSite, HasPosition, Part, Room, find, game};
use serde::{Deserialize, Serialize};

use crate::{check::{Check, Expiration, Filtered, deserialize_filter_check}, colony::ColonyView, coordination::{allocations::{CreepAllocationHandle, CreepAllocations, ResourceAmount}, tasks::{AddedToCollab, Tasks}}, creeps::{gather_energy, scrap::ScrapCreep, virtual_creep::VirtualCreep}, domain_traits::{ObjectId, ResolvableId}, ids::{CheckState, Checked, Unchecked}, goto, statemachine::{Flow, OrBreak, step, break_if}};

const MAX_IDLE_TICKS: u32 = 10;
pub const FABRICATOR_HORIZON: u32 = 300;

type TaskExpiration = Expiration<FABRICATOR_HORIZON>;

#[derive(Debug, EnumDisplay)]
#[derive_where(Serialize, Deserialize, Clone; ObjectId<ConstructionSite, S>, Expiration<MAX_IDLE_TICKS, S>, S)]
pub enum FabricatorCreep<S: CheckState = Checked> {
    Idle { ticks_left: u32, last_update_tick: u32 },
    Building(ObjectId<ConstructionSite, S>),
    Scrap(ScrapCreep)
}

impl Default for FabricatorCreep {
    fn default() -> Self { Self::idle() }
}

impl<'de> Deserialize<'de> for FabricatorCreep {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let us = FabricatorCreep::<Unchecked>::deserialize(deserializer)?;
        Ok(match us {
            FabricatorCreep::Idle { last_update_tick, ticks_left } => FabricatorCreep::Idle { last_update_tick, ticks_left },
            FabricatorCreep::Building(task) => 
                task.check().map_or_else(|_| FabricatorCreep::idle(), FabricatorCreep::Building),
            FabricatorCreep::Scrap(state) => FabricatorCreep::Scrap(state)
        })
    }
}

impl FabricatorCreep {
    fn idle() -> Self { Self::Idle { ticks_left: MAX_IDLE_TICKS, last_update_tick: game::time() } }

    pub fn update(&mut self, creep: &mut VirtualCreep, home: &ColonyView<'_>, coordinator: &mut FabricatorCoordinator) -> anyhow::Result<()> {
        *self = step(mem::take(self), |state| state.transition(creep, home, coordinator))?;
        Ok(())
    }

    fn transition(self, creep: &mut VirtualCreep, home: &ColonyView<'_>, coordinator: &mut FabricatorCoordinator) -> Flow<Self> {
        if creep.pos().room_name() != home.name {
            creep.approach(home.center, 5)?;
        }

        match self {
            Self::Idle { last_update_tick, ticks_left } => {
                goto!(if let Some(task) = coordinator.assign_build(creep) { Self::Building(task) });
                
                goto!(if last_update_tick != game::time() { Self::Idle { ticks_left: ticks_left.saturating_sub(1), last_update_tick: game::time() } });
                break_if(ticks_left > 0)?;

                Flow::Continue(FabricatorCreep::Scrap(ScrapCreep::default()))
            },
            Self::Building(task) => {
                let Some(mut handle) = coordinator.refresh(creep, &task) else { goto!(Self::idle()) };
                handle.shrink_to(creep.ticks_to_live() * creep.body().part_count(Part::Work) as u32 * BUILD_POWER);

                gather_energy(creep, || home.buffer.or_break())?;

                creep.approach(task.resolve().pos(), 1)?;
                handle.consume(creep.build(task.resolve())?);

                break_if(handle.reserved() > 0)?;

                handle.release();
                Flow::Continue(Self::idle())
            },
            FabricatorCreep::Scrap(state) => state.transition(creep, home).map_continue(FabricatorCreep::Scrap)
        }
    }
}

#[derive(Serialize, Deserialize, Default)]
pub struct FabricatorCoordinator(
    #[serde(deserialize_with = "deserialize_filter_check")] 
    Tasks<ObjectId<ConstructionSite>, Filtered<CreepAllocations<TaskExpiration>>>
);

fn hits_left(task: ObjectId<ConstructionSite>) -> u32 {
    let site = task.resolve();
    site.progress_total() - site.progress()
}

impl FabricatorCoordinator {
    pub fn update(&mut self, room: &Room) {
        self.0.set_tasks(
            room.find(find::MY_CONSTRUCTION_SITES, None).into_iter()
                .filter_map(|site| Some((
                    ObjectId::try_new(&site)?,
                    ResourceAmount(site.progress_total() - site.progress())
                )))
        );
    }

    pub fn assign_build(&mut self, creep: &VirtualCreep) -> Option<ObjectId<ConstructionSite>> {
        let build_amount = creep.ticks_to_live().min(FABRICATOR_HORIZON) * creep.body().part_count(Part::Work) as u32 * BUILD_POWER;

        self.0.iter_mut()
            .filter(|(_, collab)| collab.unreserved_amount() > 0)
            .min_by_key(|(task, _)| (hits_left(**task), creep.pos().get_range_to(task.resolve().pos())))
            .added_to_collab(creep.handle(), build_amount, Expiration::new())
    }

    pub fn total_build_work_ticks(&self) -> u32 {
        self.0.iter()
            .map(|(_, collab)| collab.ammount())
            .sum::<u32>() / BUILD_POWER
    }

    pub fn refresh(&mut self, creep: &VirtualCreep, task: &ObjectId<ConstructionSite>) -> Option<CreepAllocationHandle<'_, TaskExpiration>> {
        self.0.refresh(task, creep.handle())
    }
}