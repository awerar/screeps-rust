use screeps::{HasHits, HasPosition, Part, Position, Room, controller_downgrade, find};
use serde::{Serialize, Deserialize};
use derive_deref::Deref;

use crate::{check::{Expiry, Filtered, TriviallyChecked, deserialize_filter_check}, colony::ColonyBuffer, coordination::{collaboration::{Collaboration, CollaborativeWorkerHandle, RemainingWork}, tasks::{AddedToCollab, OverwriteableTaskData, Tasks}}, creeps::{fabricator::task::{BuildTask, FabricatorTask, RepairTask, UpgradeTask}, virtual_creep::VirtualCreep}, domain_traits::EnergyStoreAccessors, ids::{ById, IntoWithId}, structure::RepairableStructure};

macro_rules! def_f32_wrapper {
    ($name:ident) => {
        #[derive(Deref, Clone, Copy, Serialize, Deserialize, PartialEq, PartialOrd)]
        pub struct $name(pub f32);

        impl TriviallyChecked for $name {}
        impl OverwriteableTaskData for $name {}
    };
}

def_f32_wrapper!(HealthPercentage);
def_f32_wrapper!(DowngradePercentage);
def_f32_wrapper!(StorageFillPercentage);

impl OverwriteableTaskData for Option<StorageFillPercentage> {}

#[derive(Serialize, Deserialize, Default)]
pub struct FabricatorCoordinator {
    #[serde(deserialize_with = "deserialize_filter_check")] 
    repairs: Tasks<RepairTask, ((Position, HealthPercentage), Filtered<Collaboration<Expiry<(), {super::MAX_TASK_TICKS}>>>)>,
    #[serde(deserialize_with = "deserialize_filter_check")] 
    builds: Tasks<BuildTask, (Position, Filtered<Collaboration<Expiry<(), {super::MAX_TASK_TICKS}>>>)>,
    #[serde(deserialize_with = "deserialize_filter_check")] // TODO: Don't make this Tasks as it is only over one task
    upgrades: Tasks<UpgradeTask, ((DowngradePercentage, Option<StorageFillPercentage>), Filtered<Collaboration<Expiry<(), {super::MAX_TASK_TICKS}>>>)>
}

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
                    let pos = repairable.pos();
                    let health_percentage =  HealthPercentage(repairable.hits() as f32 / repairable.hits_max() as f32);

                    Some((repairable, ((pos, health_percentage), RemainingWork(damage))))
                })
        );

        self.builds.set_tasks(room.find(find::MY_CONSTRUCTION_SITES, None).into_iter()
            .map(|site| {
                (
                    ById(site.clone().with_id().unwrap()),
                    (
                        site.pos(),
                        RemainingWork(site.progress_total() - site.progress())
                    )
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
            ((DowngradePercentage(downgrade_percentage),
            storage_fill_percentage.map(StorageFillPercentage)),
            RemainingWork(u32::MAX))
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
            .filter(|(_, ((_, percentage), _))| *percentage <= super::EMERGENCY_REPAIR_PERCENTAGE)
            .min_by(|(_, ((_, p1), _)), (_, ((_, p2), _))| p1.total_cmp(p2))
            .added_to_collab(creep.handle(), get_creep_work_left(creep) * 100, Expiry::new(()))
            .or_else(|| 
                self.repairs.iter_mut()
                    .filter(|(_, ((_, percentage), _))| *percentage <= super::REPAIR_PERCENTAGE)
                    .min_by_key(|(_, ((pos, _), _))| creep.pos().get_range_to(*pos))
                    .added_to_collab(creep.handle(), get_creep_work_left(creep) * 100, Expiry::new(()))
            )
    }

    fn assign_build(&mut self, creep: &VirtualCreep) -> Option<BuildTask> {
        self.builds.iter_mut()
            .min_by_key(|(_, (pos, _))| creep.pos().get_range_to(*pos))
            .added_to_collab(creep.handle(), get_creep_work_left(creep) * 5, Expiry::new(()))
    }

    fn assign_emergency_upgrade(&mut self, creep: &VirtualCreep) -> Option<UpgradeTask> {
        self.upgrades.iter_mut()
            .find(|(_, ((percentage, _), _))| *percentage >= super::CONTROLLER_DOWNGRADE_EMERGENCY_PERCENTAGE)
            .added_to_collab(creep.handle(), get_creep_work_left(creep), Expiry::new(()))
    }

    fn assign_upgrade(&mut self, creep: &VirtualCreep) -> Option<UpgradeTask> {
        self.upgrades.iter_mut()
            .find(|(_, ((_, percentage), _))| 
                percentage.is_none_or(|percentage| percentage >= super::STORAGE_UPGRADE_CONTROLLER_THRESHOLD))
            .added_to_collab(creep.handle(), get_creep_work_left(creep), Expiry::new(()))
    }

    pub fn heartbeat(&mut self, creep: &VirtualCreep, task: &FabricatorTask) -> Option<CollaborativeWorkerHandle<'_, Expiry<(), {super::MAX_TASK_TICKS}>>> {
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