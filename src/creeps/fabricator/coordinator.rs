use screeps::{Creep, HasHits, HasPosition, Part, Position, Room, controller_downgrade, find};
use serde::{Serialize, Deserialize};

use crate::{colony::ColonyBuffer, creeps::{fabricator::{DowngradePercentage, HealthPercentage, StorageFillPercentage, task::{BuildTask, FabricatorTask, FabricatorTaskType, RepairTask, UpgradeTask}}, virtual_creep::VirtualCreep}, domain_traits::EnergyStoreAccessors, ids::{ById, Handle, IntoWithId, WithId}, structure::RepairableStructure, tasks::{TaskServer, prune_deserialize_taskserver}};

#[derive(Serialize, Deserialize, Default)]
pub struct FabricatorCoordinator {
    #[serde(deserialize_with = "prune_deserialize_taskserver")] 
    repairs: TaskServer<RepairTask, (Position, HealthPercentage)>,
    #[serde(deserialize_with = "prune_deserialize_taskserver")] 
    builds: TaskServer<BuildTask, Position>,
    #[serde(deserialize_with = "prune_deserialize_taskserver")] 
    upgrades: TaskServer<UpgradeTask, (DowngradePercentage, Option<StorageFillPercentage>)>
}

fn get_creep_work_left(creep: &VirtualCreep) -> u32 {
    let work_ticks_left = creep.ticks_to_live().unwrap().saturating_sub(super::GUESSED_CREEP_MOVE_TO_TASK_TICKS);
    let work_ticks_left = work_ticks_left.min(super::MAX_TASK_TICKS);

    let work_part_count = creep.body().num(Part::Work) as u32;
    work_ticks_left * work_part_count
}

impl FabricatorCoordinator {
    pub fn update(&mut self, room: &Room, buffer: Option<ColonyBuffer>) {
        self.repairs.set_tasks(room.find(find::STRUCTURES, None).into_iter()
            .filter_map(|structure| {
                let repairable = RepairableStructure::try_from(structure).ok()?;

                let damage = repairable.hits_max() - repairable.hits();
                let pos = repairable.pos();
                let health_percentage =  HealthPercentage(repairable.hits() as f32 / repairable.hits_max() as f32);

                Some((repairable, damage, (pos, health_percentage)))
            })
        );

        self.builds.set_tasks(room.find(find::MY_CONSTRUCTION_SITES, None).into_iter()
            .map(|site| {
                (
                    ById(site.clone().with_id().unwrap()),
                    site.progress_total() - site.progress(),
                    site.pos()
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
            u32::MAX,
            (DowngradePercentage(downgrade_percentage),
            storage_fill_percentage.map(StorageFillPercentage))
        )]);
    }

    pub fn assign_task(&mut self, creep: &VirtualCreep) -> Option<FabricatorTask> {
        self.assign_emergency_upgrade(creep).map(FabricatorTaskType::UpgradingController)
            .or_else(|| self.assign_repair(creep).map(FabricatorTaskType::Repairing))
            .or_else(|| self.assign_build(creep).map(FabricatorTaskType::Building))
            .or_else(|| self.assign_upgrade(creep).map(FabricatorTaskType::UpgradingController))
            .map(FabricatorTask::new)
    }

    fn assign_repair(&mut self, creep: &VirtualCreep) -> Option<RepairTask> {
        let contribution = get_creep_work_left(creep) * 100;
        self.repairs.assign_task(creep.handle(), contribution, |tasks| {
            let emergency_repair = tasks.clone().into_iter()
                .filter(|(_, _, (_, percentage))| *percentage <= super::EMERGENCY_REPAIR_PERCENTAGE)
                .min_by(|(_, _, (_, p1)), (_, _, (_, p2))| p1.total_cmp(p2));
            if emergency_repair.is_some() { return emergency_repair }

            tasks.into_iter()
                .filter(|(_, _, (_, percentage))| *percentage <= super::REPAIR_PERCENTAGE)
                .min_by_key(|(_, _, (pos, _))| creep.pos().get_range_to(*pos))
        })
    }

    fn assign_build(&mut self, creep: &VirtualCreep) -> Option<BuildTask> {
        let contribution = get_creep_work_left(creep) * 5;
        self.builds.assign_task(creep.handle(), contribution, |tasks| {
            tasks.into_iter()
                .min_by_key(|(_, _, pos)| creep.pos().get_range_to(**pos))
        })
    }

    fn assign_emergency_upgrade(&mut self, creep: &VirtualCreep) -> Option<UpgradeTask> {
        let contribution = get_creep_work_left(creep) * 2;
        self.upgrades.assign_task(creep.handle(), contribution, |tasks| {
            tasks.into_iter()
                .find(|(_, _, (percentage, _))| *percentage >= super::CONTROLLER_DOWNGRADE_EMERGENCY_PERCENTAGE)
        })
    }

    fn assign_upgrade(&mut self, creep: &VirtualCreep) -> Option<UpgradeTask> {
        let contribution = get_creep_work_left(creep) * 2;
        self.upgrades.assign_task(creep.handle(), contribution, |tasks| {
            tasks.into_iter()
                .find(|(_, _, (_, percentage))| 
                    percentage.is_none_or(|percentage| percentage >= super::STORAGE_UPGRADE_CONTROLLER_THRESHOLD))
        })
    }

    pub fn heartbeat_task(&mut self, creep: &Handle<WithId<Creep>>, task: &FabricatorTask) -> bool {
        match task.task_type() {
            FabricatorTaskType::Building(build) => 
                self.builds.heartbeat_task(creep, build),
            FabricatorTaskType::Repairing(repair) => 
                self.repairs.heartbeat_task(creep, repair),
            FabricatorTaskType::UpgradingController(upgrade) => 
                self.upgrades.heartbeat_task(creep, upgrade),
        }
    }

    pub fn finish_task(&mut self, creep: &Handle<WithId<Creep>>, task: &FabricatorTask, success: bool) {
        match task.task_type() {
            FabricatorTaskType::Building(build) => 
                self.builds.finish_task(creep, build, success),
            FabricatorTaskType::Repairing(repair) => 
                self.repairs.finish_task(creep, repair, success),
            FabricatorTaskType::UpgradingController(upgrade) => 
                self.upgrades.finish_task(creep, upgrade, success),
        }
    }
}