use anyhow::Result;
use derive_where::derive_where;
use enum_display::EnumDisplay;
use screeps::{BUILD_POWER, ConstructionSite, HasPosition, ResourceType, Room, find};
use serde::{Deserialize, Serialize};

use crate::{check::{Check, Expiration, Filtered, deserialize_filter_check}, colony::ColonyView, coordination::{allocations::{CreepAllocationHandle, CreepAllocations, ResourceAmount}, tasks::{AddedToCollab, Tasks}}, creeps::virtual_creep::VirtualCreep, defer, defer_err, domain_traits::{EnergyStoreAccessors, ObjectId, ResolvableId}, done, done_if, ids::{CheckState, Checked, Unchecked}, movement::requests::MovementRequests, next, statemachine::Transition};

const MAX_TASK_TICKS: u32 = 100;

type TaskExpiration = Expiration<MAX_TASK_TICKS>;

#[derive(Debug, Default, EnumDisplay)]
#[derive_where(Serialize, Deserialize, Clone; ObjectId<ConstructionSite, S>, S)]
pub enum FabricatorCreep<S: CheckState = Checked> {
    #[default] Idle,
    Building(ObjectId<ConstructionSite, S>),
}

impl<'de> Deserialize<'de> for FabricatorCreep {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let us = FabricatorCreep::<Unchecked>::deserialize(deserializer)?;
        Ok(match us {
            FabricatorCreep::Idle => Self::Idle,
            FabricatorCreep::Building(task) => 
                task.check().map_or(FabricatorCreep::Idle, FabricatorCreep::Building),
        })
    }
}

impl FabricatorCreep {
    pub fn update(self, creep: &mut VirtualCreep, home: &ColonyView<'_>, movement: &mut MovementRequests, coordinator: &mut FabricatorCoordinator) -> anyhow::Result<Transition<Self>> {
        use Transition::*;

        match self {
            Self::Idle => {
                if let Some(task) = coordinator.assign_build(creep) {
                    next!(Self::Building(task))
                }

                defer!(movement.move_vcreep_to(creep, home.center, 5), self)?;
                Ok(Done(self))
            },
            Self::Building(ref task) => {
                let Some(mut handle) = coordinator.refresh(creep, task) else { next!(Self::Idle) };

                if creep.next_used_energy_capacity() == 0 {
                    let Some(buffer) = &home.buffer else { done!(self) };
                    defer!(movement.move_vcreep_to(creep, buffer.pos(), 1), self)?;

                    done_if!(buffer.used_energy_capacity() == 0, self);
                    done_if!(creep.outgoing() > 0, self);
                    defer_err!(creep.withdraw(*buffer, ResourceType::Energy, None), self)?;
                }

                defer!(movement.move_vcreep_to(creep, task.resolve().pos(), 1), self)?;
                handle.consume(defer_err!(creep.build(task.resolve()), self)?);

                done_if!(handle.reserved() > 0, self);

                handle.release();
                Ok(Next(Self::Idle))
            }
        }
    }
}

#[derive(Serialize, Deserialize, Default)]
pub struct FabricatorCoordinator(
    #[serde(deserialize_with = "deserialize_filter_check")] 
    Tasks<ObjectId<ConstructionSite>, Filtered<CreepAllocations<TaskExpiration>>>
);

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
        self.0.iter_mut()
            .filter(|(_, collab)| collab.unreserved_amount() > 0)
            .min_by_key(|(task, _)| creep.pos().get_range_to(task.resolve().pos()))
            .added_to_collab(creep.handle(), creep.ticks_to_live() * BUILD_POWER, Expiration::new())
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