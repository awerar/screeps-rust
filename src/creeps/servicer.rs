use derive_where::derive_where;
use enum_display::EnumDisplay;
use ordered_float::OrderedFloat;
use screeps::{HasPosition, Part, REPAIR_POWER, Room, find};
use serde::{Deserialize, Serialize};

use crate::{check::{Check, Expiration, Filtered, deserialize_filter_check}, colony::ColonyView, coordination::{allocations::{CreepAllocationHandle, CreepAllocations, ResourceAmount}, tasks::{AddedToCollab, Tasks}}, creeps::{gather_energy, scrap::ScrapCreep, virtual_creep::VirtualCreep}, domain_traits::HasHits, ids::{CheckState, Checked, Unchecked}, movement::requests::MovementRequests, next, statemachine::{Step, wait_for, wait_if}, structure::RepairableStructure};

const EMERGENCY_REPAIR_PERCENTAGE: f32 = 0.5;
const REPAIR_PERCENTAGE: f32 = 0.95;
const MAX_TASK_TICKS: u32 = 100;

type TaskExpiration = Expiration<MAX_TASK_TICKS>;

#[derive(Debug, Default, EnumDisplay)]
#[derive_where(Serialize, Deserialize, Clone; RepairableStructure::<S>, S)]
pub enum ServicerCreep<S: CheckState = Checked> {
    #[default] Idle,
    Repairing(RepairableStructure::<S>),
    Scrap(ScrapCreep),
}

impl<'de> Deserialize<'de> for ServicerCreep {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let us = ServicerCreep::<Unchecked>::deserialize(deserializer)?;
        Ok(match us {
            ServicerCreep::Idle => Self::Idle,
            ServicerCreep::Repairing(task) => 
                task.check().map_or_default(ServicerCreep::Repairing),
            ServicerCreep::Scrap(state) => Self::Scrap(state)
        })
    }
}

impl ServicerCreep {
    pub fn update(&mut self, creep: &mut VirtualCreep, home: &ColonyView<'_>, movement: &mut MovementRequests, coordinator: &mut ServicerCoordinator) -> Step<Self> {

        match self {
            Self::Idle => {
                if let Some(task) = coordinator.assign_repair(creep) {
                    next!(Self::Repairing(task))
                }

                Step::Next(Self::Scrap(ScrapCreep::default()))
            },
            Self::Repairing(task) => {
                let Some(mut handle) = coordinator.refresh(creep, task) else { next!(Self::Idle) };

                gather_energy(creep, movement, || wait_for(home.buffer))?;

                movement.approach(creep, task.pos(), 3)?;
                handle.consume(creep.repair(*task)? * REPAIR_POWER);

                wait_if(handle.reserved() > 0)?;

                handle.release();
                Step::Next(Self::Idle)
            },
            Self::Scrap(state) => state.update(creep, home, movement).map(Self::Scrap)
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
        let creep_repair_power = creep.body().part_count(Part::Work) as u32 * REPAIR_POWER;
        let max_repair_amount = creep.ticks_to_live().min(MAX_TASK_TICKS) * creep_repair_power;

        self.0.iter_mut()
            .filter(|(_, collab)| collab.unreserved_amount() > 0)
            .filter(|(task, _)| health_percentage(task) <= EMERGENCY_REPAIR_PERCENTAGE)
            .min_by_key(|(task, _)| OrderedFloat(health_percentage(task)))
            .added_to_collab(creep.handle(), max_repair_amount, Expiration::new())
            .or_else(|| 
                self.0.iter_mut()
                    .filter(|(_, collab)| collab.unreserved_amount() > 0)
                    .filter(|(task, _)| health_percentage(task) <= REPAIR_PERCENTAGE)
                    .max_by_key(|(task, collab)| {
                        let repair_amount = max_repair_amount.min(collab.unreserved_amount());
                        let move_ticks = creep.pos().get_range_to(task.pos()).saturating_sub(3);
                        let repair_ticks = collab.unreserved_amount().min(max_repair_amount).div_ceil(creep_repair_power);

                        repair_amount / (move_ticks + repair_ticks)
                    }).added_to_collab(creep.handle(), max_repair_amount, Expiration::new())
            )
    }

    pub fn total_work_ticks(&self) -> u32 {
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