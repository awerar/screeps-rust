use ordered_float::OrderedFloat;
use screeps::{HasHits, HasPosition, Part, Room, controller_downgrade, find};
use serde::{Serialize, Deserialize};

use crate::{check::{Expiration, Filtered, TriviallyChecked, deserialize_filter_check}, colony::ColonyBuffer, coordination::{collaboration::{Collaboration, CollaborativeWorkerHandle, RemainingWork}, tasks::{AddedToCollab, OverwriteableTaskData, Tasks}}, creeps::{fabricator::{TaskExpiration, task::{BuildTask, FabricatorTask, RepairTask, UpgradeTask}}, virtual_creep::VirtualCreep}, domain_traits::EnergyStoreAccessors, ids::{ById, IntoWithId}, structure::RepairableStructure};

#[derive(Serialize, Deserialize, Default)]
pub struct FabricatorCoordinator {
    #[serde(deserialize_with = "deserialize_filter_check")] 
    repairs: Tasks<RepairTask, (RepairTaskData, Filtered<Collaboration<TaskExpiration>>)>,
    #[serde(deserialize_with = "deserialize_filter_check")] 
    builds: Tasks<BuildTask, Filtered<Collaboration<TaskExpiration>>>,
    #[serde(deserialize_with = "deserialize_filter_check")] // TODO: Don't make this Tasks as it is only over one task
    upgrades: Tasks<UpgradeTask, (UpgradeTaskData, Filtered<Collaboration<TaskExpiration>>)>
}

#[derive(Serialize, Deserialize)]
pub struct UpgradeTaskData {
    pub storage_fill_percentage: Option<f32>,
    pub downgrade_percentage: f32
}

impl TriviallyChecked for UpgradeTaskData {}
impl OverwriteableTaskData for UpgradeTaskData {}

#[derive(Serialize, Deserialize)]
pub struct RepairTaskData {
    health_percentage: f32
}

impl TriviallyChecked for RepairTaskData {}
impl OverwriteableTaskData for RepairTaskData {}

fn get_creep_work_left(creep: &VirtualCreep) -> u32 {
    let work_ticks_left = creep.ticks_to_live().unwrap().saturating_sub(super::GUESSED_CREEP_MOVE_TO_TASK_TICKS);
    let work_ticks_left = work_ticks_left.min(super::MAX_TASK_TICKS);

    let work_part_count = creep.body().num(Part::Work) as u32;
    work_ticks_left * work_part_count
}

impl FabricatorCoordinator {
    pub fn update(&mut self, room: &Room, buffer: Option<ColonyBuffer>) {
        self.repairs.set_tasks(
            room.find(find::STRUCTURES, None).into_iter()
                .filter_map(|structure| {
                    let repairable = RepairableStructure::try_from(structure).ok()?;

                    let damage = repairable.hits_max() - repairable.hits();
                    let health_percentage =  repairable.hits() as f32 / repairable.hits_max() as f32;

                    Some((repairable, (RepairTaskData { health_percentage }, RemainingWork(damage))))
                })
        );

        self.builds.set_tasks(room.find(find::MY_CONSTRUCTION_SITES, None).into_iter()
            .map(|site| {
                (
                    ById(site.clone().with_id().unwrap()),
                    RemainingWork(site.progress_total() - site.progress())
                )
            })
        );

        let controller = room.controller().unwrap();
        let mut downgrade_percentage = 0.0;
        if let Some(downgrade_ticks_left) = controller.ticks_to_downgrade()
            && let Some(total_downgrade_ticks) = controller_downgrade(controller.level()) {
                downgrade_percentage = (total_downgrade_ticks - downgrade_ticks_left) as f32 / total_downgrade_ticks as f32;
            }

        let storage_fill_percentage = buffer.and_then(|buffer| {
            match buffer {
                ColonyBuffer::Container(container) => {
                    let used = container.used_energy_capacity();
                    let capacity = container.energy_capacity();
                    Some(used as f32 / capacity as f32)
                },
                ColonyBuffer::Storage(_) => None,
            }
        });

        self.upgrades.set_tasks(vec![(
            ById(controller), 
            (
                UpgradeTaskData { storage_fill_percentage, downgrade_percentage },
                RemainingWork(u32::MAX)
            )
        )]);
    }

    pub fn assign_task(&mut self, creep: &VirtualCreep) -> Option<FabricatorTask> {
        self.assign_emergency_upgrade(creep).map(FabricatorTask::UpgradingController)
            .or_else(|| self.assign_repair(creep).map(FabricatorTask::Repairing))
            .or_else(|| self.assign_build(creep).map(FabricatorTask::Building))
            .or_else(|| self.assign_upgrade(creep).map(FabricatorTask::UpgradingController))
    }

    fn assign_repair(&mut self, creep: &VirtualCreep) -> Option<RepairTask> {
        self.repairs.iter_mut()
            .filter(|(_, (data, _))| data.health_percentage <= super::EMERGENCY_REPAIR_PERCENTAGE)
            .min_by_key(|(_, (data, _))| OrderedFloat(data.health_percentage))
            .added_to_collab(creep.handle(), get_creep_work_left(creep) * 100, Expiration::new())
            .or_else(|| 
                self.repairs.iter_mut()
                    .filter(|(_, (data, _))| data.health_percentage <= super::REPAIR_PERCENTAGE)
                    .min_by_key(|(task, _)| creep.pos().get_range_to(task.pos()))
                    .added_to_collab(creep.handle(), get_creep_work_left(creep) * 100, Expiration::new())
            )
    }

    fn assign_build(&mut self, creep: &VirtualCreep) -> Option<BuildTask> {
        self.builds.iter_mut()
            .min_by_key(|(task, _)| creep.pos().get_range_to(task.pos()))
            .added_to_collab(creep.handle(), get_creep_work_left(creep) * 5, Expiration::new())
    }

    fn assign_emergency_upgrade(&mut self, creep: &VirtualCreep) -> Option<UpgradeTask> {
        self.upgrades.iter_mut()
            .find(|(_, (data, _))| data.downgrade_percentage >= super::CONTROLLER_DOWNGRADE_EMERGENCY_PERCENTAGE)
            .added_to_collab(creep.handle(), get_creep_work_left(creep), Expiration::new())
    }

    fn assign_upgrade(&mut self, creep: &VirtualCreep) -> Option<UpgradeTask> {
        self.upgrades.iter_mut()
            .find(|(_, (data, _))| 
                data.storage_fill_percentage.is_none_or(|x| x >= super::STORAGE_UPGRADE_CONTROLLER_THRESHOLD))
            .added_to_collab(creep.handle(), get_creep_work_left(creep), Expiration::new())
    }

    pub fn heartbeat(&mut self, creep: &VirtualCreep, task: &FabricatorTask) -> Option<CollaborativeWorkerHandle<'_, TaskExpiration>> {
        match task {
            FabricatorTask::Building(build) => 
                self.builds.heartbeat(build, creep.handle()),
            FabricatorTask::Repairing(repair) => 
                self.repairs.heartbeat(repair, creep.handle()),
            FabricatorTask::UpgradingController(upgrade) => 
                self.upgrades.heartbeat(upgrade, creep.handle()),
        }
    }
}