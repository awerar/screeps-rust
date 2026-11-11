use anyhow::Result;
use derive_where::derive_where;
use enum_display::EnumDisplay;
use screeps::{BUILD_POWER, ConstructionSite, HasPosition, Part, Room, find};
use serde::{Deserialize, Serialize};

use crate::{check::{Check, Expiration, Filtered, deserialize_filter_check}, colony::ColonyView, coordination::{allocations::{CreepAllocationHandle, CreepAllocations, ResourceAmount}, tasks::{AddedToCollab, Tasks}}, creeps::{gather_energy, scrap::ScrapCreep, virtual_creep::VirtualCreep}, domain_traits::{ObjectId, ResolvableId}, ids::{CheckState, Checked, Unchecked}, goto, statemachine::{Flow, OrBreak, step, break_if}};

const MAX_TASK_TICKS: u32 = 100;

type TaskExpiration = Expiration<MAX_TASK_TICKS>;

#[derive(Debug, Default, EnumDisplay)]
#[derive_where(Serialize, Deserialize, Clone; ObjectId<ConstructionSite, S>, S)]
pub enum FabricatorCreep<S: CheckState = Checked> {
    #[default] Idle,
    Building(ObjectId<ConstructionSite, S>),
    Scrap(ScrapCreep)
}

impl<'de> Deserialize<'de> for FabricatorCreep {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let us = FabricatorCreep::<Unchecked>::deserialize(deserializer)?;
        Ok(match us {
            FabricatorCreep::Idle => Self::Idle,
            FabricatorCreep::Building(task) => 
                task.check().map_or(FabricatorCreep::Idle, FabricatorCreep::Building),
            FabricatorCreep::Scrap(state) => FabricatorCreep::Scrap(state)
        })
    }
}

impl FabricatorCreep {
    pub fn update(&mut self, creep: &mut VirtualCreep, home: &ColonyView<'_>, coordinator: &mut FabricatorCoordinator) {
        step(self, |state| state.transition(creep, home, coordinator));
    }

    fn transition(&mut self, creep: &mut VirtualCreep, home: &ColonyView<'_>, coordinator: &mut FabricatorCoordinator) -> Flow<Self> {
        match self {
            Self::Idle => {
                goto!(if let Some(task) = coordinator.assign_build(creep) { Self::Building(task) });

                Flow::Continue(Self::Scrap(ScrapCreep::default()))
            },
            Self::Building(task) => {
                let Some(mut handle) = coordinator.refresh(creep, task) else { goto!(Self::Idle) };

                gather_energy(creep, || home.buffer.or_break())?;

                creep.approach(task.resolve().pos(), 1)?;
                handle.consume(creep.build(task.resolve())?);

                break_if(handle.reserved() > 0)?;

                handle.release();
                Flow::Continue(Self::Idle)
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
        let build_amount = creep.ticks_to_live().min(MAX_TASK_TICKS) * creep.body().part_count(Part::Work) as u32 * BUILD_POWER;

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