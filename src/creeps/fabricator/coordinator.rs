use ordered_float::OrderedFloat;
use screeps::{BUILD_POWER, HasPosition, Part, REPAIR_POWER, Room, find};
use serde::{Serialize, Deserialize};

use crate::{check::{Expiration, Filtered, deserialize_filter_check}, coordination::{allocations::{CreepAllocationHandle, CreepAllocations, ResourceAmount}, tasks::{AddedToCollab, Tasks}}, creeps::{fabricator::{TaskExpiration, task::{BuildTask, RepairTask, StructureTask}}, virtual_creep::VirtualCreep}, domain_traits::{HasHits, ObjectId, ResolvableId}, structure::RepairableStructure};

#[derive(Serialize, Deserialize)]
#[derive(Default)]
pub struct FabricatorCoordinator {
    #[serde(deserialize_with = "deserialize_filter_check")] 
    pub repairs: Tasks<RepairTask, Filtered<CreepAllocations<TaskExpiration>>>,
    #[serde(deserialize_with = "deserialize_filter_check")] 
    pub builds: Tasks<BuildTask, Filtered<CreepAllocations<TaskExpiration>>>
}


impl VirtualCreep {
    fn estimated_work_capacity(&self) -> u32 {
        let work_ticks_left = self.ticks_to_live().unwrap().saturating_sub(super::GUESSED_CREEP_MOVE_TO_TASK_TICKS);
        let work_ticks_left = work_ticks_left.min(super::MAX_TASK_TICKS);

        let work_part_count = self.body().part_count(Part::Work) as u32;
        work_ticks_left * work_part_count
    }
}

fn health_percentage(task: &RepairTask) -> f32 {
    task.hits() as f32 / task.hits_max() as f32
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
    }

    pub fn assign_task(&mut self, creep: &VirtualCreep) -> Option<StructureTask> {
            self.assign_repair(creep).map(StructureTask::Repairing)
            .or_else(|| self.assign_build(creep).map(StructureTask::Building))
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

    pub fn refresh(&mut self, creep: &VirtualCreep, task: &StructureTask) -> Option<CreepAllocationHandle<'_, TaskExpiration>> {
        match task {
            StructureTask::Building(build) => 
                self.builds.refresh(build, creep.handle()),
            StructureTask::Repairing(repair) => 
                self.repairs.refresh(repair, creep.handle()),
        }
    }
}