use derive_where::derive_where;
use enum_display::EnumDisplay;
use ordered_float::OrderedFloat;
use screeps::{HasPosition, Part, REPAIR_POWER, ResourceType, Room, find};
use serde::{Deserialize, Serialize};

use crate::{check::{Check, Expiration, Filtered, deserialize_filter_check}, colony::ColonyView, coordination::{allocations::{CreepAllocationHandle, CreepAllocations, ResourceAmount}, tasks::{AddedToCollab, Tasks}}, creeps::virtual_creep::VirtualCreep, defer, defer_err, domain_traits::{EnergyStoreAccessors, HasHits}, done, done_if, ids::{CheckState, Checked, Unchecked}, movement::requests::MovementRequests, next, statemachine::Transition, structure::RepairableStructure};

const REPAIR_PERCENTAGE: f32 = 0.75;
const EMERGENCY_REPAIR_PERCENTAGE: f32 = 0.5;
const MAX_TASK_TICKS: u32 = 100;

type TaskExpiration = Expiration<MAX_TASK_TICKS>;

#[derive(Debug, Default, EnumDisplay)]
#[derive_where(Serialize, Deserialize, Clone; RepairableStructure::<S>, S)]
pub enum ServicerCreep<S: CheckState = Checked> {
    #[default] Idle,
    Repairing(RepairableStructure::<S>),
}

impl<'de> Deserialize<'de> for ServicerCreep {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let us = ServicerCreep::<Unchecked>::deserialize(deserializer)?;
        Ok(match us {
            ServicerCreep::Idle => Self::Idle,
            ServicerCreep::Repairing(task) => 
                task.check().map_or_default(ServicerCreep::Repairing),
        })
    }
}

impl ServicerCreep {
    pub fn update(self, creep: &mut VirtualCreep, home: &ColonyView<'_>, movement: &mut MovementRequests, coordinator: &mut ServicerCoordinator) -> anyhow::Result<Transition<Self>> {
        use Transition::*;

        match self {
            Self::Idle => {
                if let Some(task) = coordinator.assign_repair(creep) {
                    next!(Self::Repairing(task))
                }

                defer!(movement.move_vcreep_to(creep, home.center, 5), self)?;
                Ok(Done(self))
            },
            Self::Repairing(ref task) => {
                let Some(mut handle) = coordinator.refresh(creep, task) else { next!(Self::Idle) };

                if creep.next_used_energy_capacity() == 0 {
                    let Some(buffer) = &home.buffer else { done!(self) };
                    defer!(movement.move_vcreep_to(creep, buffer.pos(), 1), self)?;

                    done_if!(buffer.used_energy_capacity() == 0, self);
                    done_if!(creep.outgoing() > 0, self);
                    defer_err!(creep.withdraw(*buffer, ResourceType::Energy, None), self)?;
                }

                defer!(movement.move_vcreep_to(creep, task.pos(), 1), self)?;
                handle.consume(defer_err!(creep.repair(*task), self)? * REPAIR_POWER);

                done_if!(handle.reserved() > 0, self);

                handle.release();
                Ok(Next(Self::Idle))
            }
        }
    }
}

#[derive(Serialize, Deserialize)]
#[derive(Default)]
pub struct ServicerCoordinator(
    #[serde(deserialize_with = "deserialize_filter_check")] 
    Tasks<RepairableStructure, Filtered<CreepAllocations<TaskExpiration>>>,
);

fn health_percentage(task: &RepairableStructure) -> f32 {
    task.hits() as f32 / task.hits_max() as f32
}

impl ServicerCoordinator {
    pub fn update(&mut self, room: &Room) {
        self.0.set_tasks(
            room.find(find::STRUCTURES, None).into_iter()
                .filter_map(|structure| {
                    let repairable = RepairableStructure::try_from(structure).ok()?;
                    let damage = repairable.hits_max() - repairable.hits();

                    Some((repairable, ResourceAmount(damage)))
                }).filter(|(_, dmg)| dmg.0 > 0)
        );
    }

    pub fn assign_repair(&mut self, creep: &VirtualCreep) -> Option<RepairableStructure> {
        let repair_amount = creep.ticks_to_live().min(MAX_TASK_TICKS) * creep.body().part_count(Part::Work) as u32 * REPAIR_POWER;

        self.0.iter_mut()
            .filter(|(_, collab)| collab.unreserved_amount() > 0)
            .filter(|(task, _)| health_percentage(task) <= EMERGENCY_REPAIR_PERCENTAGE)
            .min_by_key(|(task, _)| OrderedFloat(health_percentage(task)))
            .added_to_collab(creep.handle(), repair_amount, Expiration::new())
            .or_else(|| 
                self.0.iter_mut()
                    .filter(|(_, collab)| collab.unreserved_amount() > 0)
                    .filter(|(task, _)| health_percentage(task) <= REPAIR_PERCENTAGE)
                    .min_by_key(|(task, _)| creep.pos().get_range_to(task.pos()))
                    .added_to_collab(creep.handle(), repair_amount, Expiration::new())
            )
    }

    pub fn total_repair_work_ticks(&self) -> u32 {
        self.0.iter()
            .filter(|(task, _)| health_percentage(task) <= REPAIR_PERCENTAGE)
            .map(|(_, collab)| collab.0.ammount())
            .sum::<u32>() / REPAIR_POWER
    }

    pub fn any_emergency_repair(&self) -> bool {
        self.0.iter()
            .any(|(task, _)| health_percentage(task) <= EMERGENCY_REPAIR_PERCENTAGE)
    }

    pub fn refresh(&mut self, creep: &VirtualCreep, task: &RepairableStructure) -> Option<CreepAllocationHandle<'_, TaskExpiration>> {
        self.0.refresh(task, creep.handle())
    }
}