use ordered_float::OrderedFloat;
use screeps::{HasHits, HasPosition, Part, Room, StructureController, controller_downgrade, find};
use serde::{Serialize, Deserialize};

use crate::{check::{Expiration, Filtered, deserialize_filter_check}, colony::{ColonyBuffer, ColonyView}, coordination::{allocations::{CreepAllocationHandle, CreepAllocations, ResourceAmount}, expiring_map::{ExpiringCreepMap, LiveCreepHandle}, tasks::{AddedToCollab, Tasks}}, creeps::{fabricator::{TaskExpiration, task::{BuildTask, FabricatorTask, RepairTask}}, virtual_creep::VirtualCreep}, domain_traits::EnergyStoreAccessors, ids::{ById, IntoWithId}, structure::RepairableStructure};

#[derive(Serialize, Deserialize, Default)]
pub struct FabricatorCoordinator {
    #[serde(deserialize_with = "deserialize_filter_check")] 
    pub repairs: Tasks<RepairTask, Filtered<CreepAllocations<TaskExpiration>>>,
    #[serde(deserialize_with = "deserialize_filter_check")] 
    pub builds: Tasks<BuildTask, Filtered<CreepAllocations<TaskExpiration>>>,
    #[serde(deserialize_with = "deserialize_filter_check")]
    pub upgrade: ExpiringCreepMap<TaskExpiration> // Make workers have to reserve a portion of the tick upgrade budget
}

pub enum FabricatorTaskHandle<'a> {
    Collab(CreepAllocationHandle<'a, TaskExpiration>),
    Upgrade(LiveCreepHandle<'a, TaskExpiration>)
}

fn get_creep_work_left(creep: &VirtualCreep) -> u32 {
    let work_ticks_left = creep.ticks_to_live().unwrap().saturating_sub(super::GUESSED_CREEP_MOVE_TO_TASK_TICKS);
    let work_ticks_left = work_ticks_left.min(super::MAX_TASK_TICKS);

    let work_part_count = creep.body().num(Part::Work) as u32;
    work_ticks_left * work_part_count
}

fn health_percentage(task: &RepairTask) -> f32 {
    task.hits() as f32 / task.hits_max() as f32
}


fn downgrade_percentage(controller: &StructureController) -> f32 {
    let Some(downgrade_ticks_left) = controller.ticks_to_downgrade() else { return 0.0 };
    let Some(total_downgrade_ticks) = controller_downgrade(controller.level()) else { return 0.0 };

    (total_downgrade_ticks - downgrade_ticks_left) as f32 / total_downgrade_ticks as f32
}

fn storage_fill_percentage(buffer: Option<&ColonyBuffer>) -> Option<f32> {
    buffer.and_then(|buffer| {
        match buffer {
            ColonyBuffer::Container(container) => {
                let used = container.used_energy_capacity();
                let capacity = container.energy_capacity();
                Some(used as f32 / capacity as f32)
            },
            ColonyBuffer::Storage(_) => None,
        }
    })
}

impl FabricatorCoordinator {
    pub fn update(&mut self, room: &Room) {
        self.repairs.set_tasks(
            room.find(find::STRUCTURES, None).into_iter()
                .filter_map(|structure| {
                    let repairable = RepairableStructure::try_from(structure).ok()?;
                    let damage = repairable.hits_max() - repairable.hits();

                    Some((repairable, ResourceAmount(damage)))
                })
        );

        self.builds.set_tasks(
            room.find(find::MY_CONSTRUCTION_SITES, None).into_iter()
                .map(|site| (
                    ById(site.clone().with_id().unwrap()),
                    ResourceAmount(site.progress_total() - site.progress())
                ))
        );
    }

    pub fn assign_task(&mut self, creep: &VirtualCreep, home: &ColonyView<'_>) -> Option<FabricatorTask> {
        self.assign_emergency_upgrade(creep, home).then_some(FabricatorTask::UpgradingController)
            .or_else(|| self.assign_repair(creep).map(FabricatorTask::Repairing))
            .or_else(|| self.assign_build(creep).map(FabricatorTask::Building))
            .or_else(|| self.assign_upgrade(creep, home).then_some(FabricatorTask::UpgradingController))
    }

    fn assign_repair(&mut self, creep: &VirtualCreep) -> Option<RepairTask> {
        self.repairs.iter_mut()
            .filter(|(task, _)| health_percentage(task) <= super::EMERGENCY_REPAIR_PERCENTAGE)
            .min_by_key(|(task, _)| OrderedFloat(health_percentage(task)))
            .added_to_collab(creep.handle(), get_creep_work_left(creep) * 100, Expiration::new())
            .or_else(|| 
                self.repairs.iter_mut()
                    .filter(|(task, _)| health_percentage(task) <= super::REPAIR_PERCENTAGE)
                    .min_by_key(|(task, _)| creep.pos().get_range_to(task.pos()))
                    .added_to_collab(creep.handle(), get_creep_work_left(creep) * 100, Expiration::new())
            )
    }

    fn assign_build(&mut self, creep: &VirtualCreep) -> Option<BuildTask> {
        self.builds.iter_mut()
            .min_by_key(|(task, _)| creep.pos().get_range_to(task.pos()))
            .added_to_collab(creep.handle(), get_creep_work_left(creep) * 5, Expiration::new())
    }

    fn assign_emergency_upgrade(&mut self, creep: &VirtualCreep, home: &ColonyView<'_>) -> bool {
        if downgrade_percentage(&home.controller) >= super::CONTROLLER_DOWNGRADE_EMERGENCY_PERCENTAGE {
            self.upgrade.insert(creep.handle(), Expiration::new());
            return true;
        }

        false
    }

    fn assign_upgrade(&mut self, creep: &VirtualCreep, home: &ColonyView<'_>) -> bool {
        if storage_fill_percentage(home.buffer.as_ref()).is_none_or(|x| x >= super::STORAGE_UPGRADE_CONTROLLER_THRESHOLD) {
            self.upgrade.insert(creep.handle(), Expiration::new());
            return true;
        }

        false
    }

    pub fn heartbeat(&mut self, creep: &VirtualCreep, task: &FabricatorTask) -> Option<FabricatorTaskHandle<'_>> {
        match task {
            FabricatorTask::Building(build) => 
                self.builds.heartbeat(build, creep.handle()).map(FabricatorTaskHandle::Collab),
            FabricatorTask::Repairing(repair) => 
                self.repairs.heartbeat(repair, creep.handle()).map(FabricatorTaskHandle::Collab),
            FabricatorTask::UpgradingController => 
                self.upgrade.refresh(creep.handle()).map(FabricatorTaskHandle::Upgrade),
        }
    }
}

impl FabricatorTaskHandle<'_> {
    pub fn remove(self) {
        match self {
            FabricatorTaskHandle::Collab(handle) => handle.release(),
            FabricatorTaskHandle::Upgrade(handle) => handle.remove(),
        }
    }
}