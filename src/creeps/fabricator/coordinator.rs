use ordered_float::OrderedFloat;
use screeps::{BUILD_POWER, CONTROLLER_MAX_UPGRADE_PER_TICK, HasPosition, Part, REPAIR_POWER, Room, StructureController, UPGRADE_CONTROLLER_POWER, controller_downgrade, find};
use serde::{Serialize, Deserialize};

use crate::{check::{Expiration, Filtered, deserialize_filter_check}, colony::{ColonyBuffer, ColonyView}, coordination::{allocations::{CreepAllocationHandle, CreepAllocations, ResourceAmount}, tasks::{AddedToCollab, Tasks}}, creeps::{fabricator::{TaskExpiration, task::{BuildTask, FabricatorTask, RepairTask, StructureTask}}, virtual_creep::VirtualCreep}, domain_traits::{EnergyStoreAccessors, HasHits, ObjectId, ResolvableId}, structure::RepairableStructure};

#[derive(Serialize, Deserialize)]
pub struct FabricatorCoordinator {
    #[serde(deserialize_with = "deserialize_filter_check")] 
    pub repairs: Tasks<RepairTask, Filtered<CreepAllocations<TaskExpiration>>>,
    #[serde(deserialize_with = "deserialize_filter_check")] 
    pub builds: Tasks<BuildTask, Filtered<CreepAllocations<TaskExpiration>>>,
    #[serde(deserialize_with = "deserialize_filter_check")]
    pub upgrade: CreepAllocations<TaskExpiration>
}

impl Default for FabricatorCoordinator {
    fn default() -> Self {
        Self { 
            repairs: Tasks::default(), 
            builds: Tasks::default(), 
            upgrade: CreepAllocations::new(0)
        }
    }
}

impl VirtualCreep {
    fn estimated_work_capacity(&self) -> u32 {
        let work_ticks_left = self.ticks_to_live().unwrap().saturating_sub(super::GUESSED_CREEP_MOVE_TO_TASK_TICKS);
        let work_ticks_left = work_ticks_left.min(super::MAX_TASK_TICKS);

        let work_part_count = self.body().num(Part::Work) as u32;
        work_ticks_left * work_part_count
    }
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
        let storage = buffer.resolve_storage()?;
        Some(storage.used_energy_capacity() as f32 / storage.energy_capacity() as f32)
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
                .filter_map(|site| Some((
                    ObjectId::try_new(&site)?,
                    ResourceAmount(site.progress_total() - site.progress())
                )))
        );

        self.upgrade.set_amount(
            if room.controller().is_some_and(|controller| controller.level() == 8) { 
                CONTROLLER_MAX_UPGRADE_PER_TICK
            } else { 
                u32::MAX 
            }
        );
    }

    pub fn assign_task(&mut self, creep: &VirtualCreep, home: &ColonyView<'_>) -> Option<FabricatorTask> {
        self.assign_emergency_upgrade(creep, home).then_some(FabricatorTask::Upgrading)
            .or_else(|| self.assign_repair(creep).map(StructureTask::Repairing).map(FabricatorTask::Structure))
            .or_else(|| self.assign_build(creep).map(StructureTask::Building).map(FabricatorTask::Structure))
            .or_else(|| self.assign_upgrade(creep, home).then_some(FabricatorTask::Upgrading))
    }

    fn assign_repair(&mut self, creep: &VirtualCreep) -> Option<RepairTask> {
        self.repairs.iter_mut()
            .filter(|(_, collab)| collab.unreserved_amount() > 0)
            .filter(|(task, _)| health_percentage(task) <= super::EMERGENCY_REPAIR_PERCENTAGE)
            .min_by_key(|(task, _)| OrderedFloat(health_percentage(task)))
            .added_to_collab(creep.handle(), creep.estimated_work_capacity() * REPAIR_POWER, Expiration::new())
            .or_else(|| 
                self.repairs.iter_mut()
                    .filter(|(_, collab)| collab.unreserved_amount() > 0)
                    .filter(|(task, _)| health_percentage(task) <= super::REPAIR_PERCENTAGE)
                    .min_by_key(|(task, _)| creep.pos().get_range_to(task.pos()))
                    .added_to_collab(creep.handle(), creep.estimated_work_capacity() * REPAIR_POWER, Expiration::new())
            )
    }

    fn assign_build(&mut self, creep: &VirtualCreep) -> Option<BuildTask> {
        self.builds.iter_mut()
            .filter(|(_, collab)| collab.unreserved_amount() > 0)
            .min_by_key(|(task, _)| creep.pos().get_range_to(task.resolve().pos()))
            .added_to_collab(creep.handle(), creep.estimated_work_capacity() * BUILD_POWER, Expiration::new())
    }

    fn assign_emergency_upgrade(&mut self, creep: &VirtualCreep, home: &ColonyView<'_>) -> bool {
        if self.upgrade.unreserved_amount() > 0 && downgrade_percentage(&home.controller) >= super::CONTROLLER_DOWNGRADE_EMERGENCY_PERCENTAGE {
            self.upgrade.allocate(creep.handle(), creep.body().num(Part::Work) as u32 * UPGRADE_CONTROLLER_POWER, Expiration::new());
            return true;
        }

        false
    }

    fn assign_upgrade(&mut self, creep: &VirtualCreep, home: &ColonyView<'_>) -> bool {
        if self.upgrade.unreserved_amount() > 0 && storage_fill_percentage(home.buffer.as_ref()).is_none_or(|x| x >= super::STORAGE_UPGRADE_CONTROLLER_THRESHOLD) {
            self.upgrade.allocate(creep.handle(), creep.body().num(Part::Work) as u32, Expiration::new());
            return true;
        }

        false
    }

    pub fn refresh_structure(&mut self, creep: &VirtualCreep, task: &StructureTask) -> Option<CreepAllocationHandle<'_, TaskExpiration>> {
        match task {
            StructureTask::Building(build) => 
                self.builds.refresh(build, creep.handle()),
            StructureTask::Repairing(repair) => 
                self.repairs.refresh(repair, creep.handle()),
        }
    }

    pub fn refresh(&mut self, creep: &VirtualCreep, task: &FabricatorTask) -> Option<CreepAllocationHandle<'_, TaskExpiration>> {
        match task {
            FabricatorTask::Structure(task) => 
                self.refresh_structure(creep, task),
            FabricatorTask::Upgrading => 
                self.upgrade.refresh(creep.handle()),
        }
    }
}