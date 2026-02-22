use derive_deref::Deref;
use screeps::{ConstructionSite, Creep, HasId, HasPosition, MaybeHasId, ObjectId, Part, Position, ResourceType, Room, SharedCreepProperties, Structure, StructureController, StructureObject, controller_downgrade, find, game};
use serde::{Serialize, Deserialize};
use derive_alias::derive_alias;

use crate::{colony::ColonyBuffer, memory::Memory, messages::{CreepMessage, TruckMessage}, statemachine::{StateMachine, Transition}, tasks::TaskServer};

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub enum FabricatorCreep {
    #[default] Idle,
    CollectingFor(FabricatorTask),
    Performing(FabricatorTask)
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum FabricatorTaskType {
    Building(BuildTask),
    Repairing(RepairTask),
    UpgradingController(UpgradeTask)
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FabricatorTask {
    task_type: FabricatorTaskType,
    start_time: u32,
    pos: Position
}

derive_alias! {
    derive_percentage => #[derive(Deref, Clone, Copy, Serialize, Deserialize, PartialEq, PartialOrd)]
}

derive_percentage! { struct HealthPercentage(f32); }
derive_percentage! { struct DowngradePercentage(f32); }
derive_percentage! { struct StorageFillPercentage(f32); }

type BuildTask = ObjectId<ConstructionSite>;
type RepairTask = ObjectId<Structure>;
type UpgradeTask = ObjectId<StructureController>;

const REPAIR_PERCENTAGE: HealthPercentage = HealthPercentage(0.75);
const EMERGENCY_REPAIR_PERCENTAGE: HealthPercentage = HealthPercentage(0.5);
const CONTROLLER_DOWNGRADE_EMERGENCY_PERCENTAGE: DowngradePercentage = DowngradePercentage(0.5);
const STORAGE_UPGRADE_CONTROLLER_THRESHOLD: StorageFillPercentage = StorageFillPercentage(0.1);

const MAX_TASK_TICKS: u32 = 100;
const GUESSED_CREEP_MOVE_TO_TASK_TICKS: u32 = 50;

impl StateMachine<Creep> for FabricatorCreep {
    fn update(&self, creep: &Creep, mem: &mut Memory) -> Result<Transition<Self>, ()> {
        use Transition::*;

        let home = mem.creep(creep).unwrap().home;
        let coordinator = mem.fabricator_coordinators.entry(home).or_default();

        match self {
            Self::Idle => {
                let task = coordinator.assign_task(creep);
                if let Some(task) = task {
                    return Ok(Continue(Self::Performing(task)))
                }

                mem.messages.trucks.send(TruckMessage::Provider(creep.try_id().unwrap(), home));
                Ok(Stay)
            },
            Self::CollectingFor(task) => {
                if task.has_timed_out() || !coordinator.heartbeat_task(creep, task) {
                    coordinator.finish_task(creep, task, false);
                    return Ok(Break(Self::Idle))
                }

                if mem.messages.creep(creep).read(CreepMessage::TruckTarget) 
                    || creep.store().get_used_capacity(Some(ResourceType::Energy)) > 0 {
                        return Ok(Continue(Self::Performing(task.clone())))
                    }

                mem.messages.trucks.send(TruckMessage::Consumer(creep.try_id().unwrap(), home));

                let buffer = mem.creep_home(creep).ok_or(())?.buffer();
                let Some(buffer) = buffer else {
                    return Ok(Break(Self::CollectingFor(task.clone())))
                };

                if creep.pos().is_near_to(buffer.pos()) {
                    if buffer.store().get_used_capacity(Some(ResourceType::Energy)) > 0 {
                        creep.withdraw(buffer.withdrawable(), ResourceType::Energy, None).map_err(|_| ())?;
                        Ok(Break(Self::Performing(task.clone())))
                    } else { Ok(Stay) }
                } else {
                    mem.movement.smart_move_creep_to(creep, buffer.pos()).ok();
                    Ok(Stay)
                }
            },
            Self::Performing(task) => {
                if task.has_timed_out() || !coordinator.heartbeat_task(creep, task) {
                    coordinator.finish_task(creep, task, false);
                    return Ok(Continue(Self::Idle))
                }

                let creep_energy = creep.store().get_used_capacity(Some(ResourceType::Energy));
                if creep_energy == 0 && !mem.messages.creep(creep).read(CreepMessage::TruckTarget) {
                    return Ok(Continue(Self::CollectingFor(task.clone())))
                }

                mem.messages.trucks.send(TruckMessage::Consumer(creep.try_id().unwrap(), home));

                if !creep.pos().is_near_to(task.pos) {
                    mem.movement.smart_move_creep_to(creep, task.pos).ok();
                }

                if creep_energy > 0 && creep.pos().get_range_to(task.pos) <= task.work_range() {
                    task.creep_work(creep)?;
                }

                Ok(Stay)
            }
        }
    }
}

impl FabricatorTask {
    fn new(task_type: FabricatorTaskType) -> Option<Self> {
        Some(Self {
            start_time: game::time(),
            pos: task_type.try_pos()?,
            task_type,
        })
    }

    fn has_timed_out(&self) -> bool {
        game::time() >= self.start_time + MAX_TASK_TICKS
    }

    fn work_range(&self) -> u32 {
        match self.task_type {
            FabricatorTaskType::Building(_) | FabricatorTaskType::Repairing(_) => 1,
            FabricatorTaskType::UpgradingController(_) => 3,
        }
    }

    fn creep_work(&self, creep: &Creep) -> Result<(), ()> {
        match self.task_type {
            FabricatorTaskType::Building(site) => 
                creep.build(&site.resolve().ok_or(())?).map_err(|_| ()),
            FabricatorTaskType::Repairing(structure) => {
                let structure_object = StructureObject::from(structure.resolve().ok_or(())?);
                let repairable = structure_object.as_repairable().ok_or(())?;
                creep.repair(repairable).map_err(|_| ())
            },
            FabricatorTaskType::UpgradingController(controller) => 
                creep.upgrade_controller(&controller.resolve().ok_or(())?).map_err(|_| ()),
        }
    }
}

impl FabricatorTaskType {
    fn try_pos(&self) -> Option<Position> {
        Some(match self {
            FabricatorTaskType::Building(id) => id.resolve()?.pos(),
            FabricatorTaskType::Repairing(id) => id.resolve()?.pos(),
            FabricatorTaskType::UpgradingController(id) => id.resolve()?.pos(),
        })
    }
}

fn get_creep_work_count(creep: &Creep) -> u32 {
    let work_ticks_left = creep.ticks_to_live().unwrap().saturating_sub(GUESSED_CREEP_MOVE_TO_TASK_TICKS);
    let work_ticks_left = work_ticks_left.min(MAX_TASK_TICKS);

    let work_part_count = creep.body().iter().filter(|bodypart| bodypart.part() == Part::Work).count() as u32;
    work_ticks_left * work_part_count
}

#[derive(Serialize, Deserialize, Default)]
pub struct FabricatorCoordinator {
    repairs: TaskServer<RepairTask, (Position, HealthPercentage)>,
    builds: TaskServer<BuildTask, Position>,
    upgrades: TaskServer<UpgradeTask, (DowngradePercentage, Option<StorageFillPercentage>)>
}

impl FabricatorCoordinator {
    pub fn update(&mut self, room: &Room, buffer: Option<ColonyBuffer>) {
        self.repairs.set_tasks(room.find(find::STRUCTURES, None).into_iter()
            .filter_map(|structure| {
                let repairable = structure.as_repairable()?;
                Some((
                    structure.as_structure().id(), 
                    repairable.hits_max() - repairable.hits(),
                    (structure.pos(), 
                    HealthPercentage(repairable.hits() as f32 / repairable.hits_max() as f32))
                ))
            })
        );

        self.builds.set_tasks(room.find(find::MY_CONSTRUCTION_SITES, None).into_iter()
            .map(|site| {
                (
                    site.try_id().unwrap(),
                    site.progress_total() - site.progress(),
                    site.pos()
                )
            })
        );

        let controller = room.controller().unwrap();
        let mut downgrade_percentage = 0.0;
        if let Some(downgrade_ticks_left) = controller.ticks_to_downgrade() {
            if let Some(total_downgrade_ticks) = controller_downgrade(controller.level()) {
                downgrade_percentage = (total_downgrade_ticks - downgrade_ticks_left) as f32 / total_downgrade_ticks as f32;
            }
        }

        let storage_fill_percentage = buffer.and_then(|buffer| {
            match buffer {
                ColonyBuffer::Container(container) => {
                    let used = container.store().get_used_capacity(Some(ResourceType::Energy));
                    let capacity = container.store().get_capacity(Some(ResourceType::Energy));
                    Some(used as f32 / capacity as f32)
                },
                ColonyBuffer::Storage(_) => None,
            }
        });

        self.upgrades.set_tasks(vec![(
            controller.id(), 
            u32::MAX,
            (DowngradePercentage(downgrade_percentage),
            storage_fill_percentage.map(StorageFillPercentage))
        )]);
    }

    fn assign_task(&mut self, creep: &Creep) -> Option<FabricatorTask> {
        self.assign_emergency_upgrade(creep).map(FabricatorTaskType::UpgradingController)
            .or_else(|| self.assign_repair(creep).map(FabricatorTaskType::Repairing))
            .or_else(|| self.assign_build(creep).map(FabricatorTaskType::Building))
            .or_else(|| self.assign_upgrade(creep).map(FabricatorTaskType::UpgradingController))
            .and_then(FabricatorTask::new)
    }

    fn assign_repair(&mut self, creep: &Creep) -> Option<RepairTask> {
        let contribution = get_creep_work_count(creep) * 100;
        self.repairs.assign_task(creep, contribution, |tasks| {
            let emergency_repair = tasks.clone().into_iter()
                .filter(|(_, _, (_, percentage))| *percentage <= EMERGENCY_REPAIR_PERCENTAGE)
                .min_by(|(_, _, (_, p1)), (_, _, (_, p2))| p1.total_cmp(p2));
            if emergency_repair.is_some() { return emergency_repair }

            tasks.into_iter()
                .filter(|(_, _, (_, percentage))| *percentage <= REPAIR_PERCENTAGE)
                .min_by_key(|(_, _, (pos, _))| creep.pos().get_range_to(*pos))
        })
    }

    fn assign_build(&mut self, creep: &Creep) -> Option<BuildTask> {
        let contribution = get_creep_work_count(creep) * 5;
        self.builds.assign_task(creep, contribution, |tasks| {
            tasks.into_iter()
                .min_by_key(|(_, _, pos)| creep.pos().get_range_to(**pos))
        })
    }

    fn assign_emergency_upgrade(&mut self, creep: &Creep) -> Option<UpgradeTask> {
        let contribution = get_creep_work_count(creep) * 2;
        self.upgrades.assign_task(creep, contribution, |tasks| {
            tasks.into_iter()
                .find(|(_, _, (percentage, _))| *percentage >= CONTROLLER_DOWNGRADE_EMERGENCY_PERCENTAGE)
        })
    }

    fn assign_upgrade(&mut self, creep: &Creep) -> Option<UpgradeTask> {
        let contribution = get_creep_work_count(creep) * 2;
        self.upgrades.assign_task(creep, contribution, |tasks| {
            tasks.into_iter()
                .find(|(_, _, (_, percentage))| 
                    percentage.is_none_or(|percentage| percentage >= STORAGE_UPGRADE_CONTROLLER_THRESHOLD))
        })
    }

    fn heartbeat_task(&mut self, creep: &Creep, task: &FabricatorTask) -> bool {
        match task.task_type {
            FabricatorTaskType::Building(build) => 
                self.builds.heartbeat_task(creep, &build),
            FabricatorTaskType::Repairing(repair) => 
                self.repairs.heartbeat_task(creep, &repair),
            FabricatorTaskType::UpgradingController(upgrade) => 
                self.upgrades.heartbeat_task(creep, &upgrade),
        }
    }

    fn finish_task(&mut self, creep: &Creep, task: &FabricatorTask, success: bool) {
        let creep_id = creep.try_id().unwrap();

        match task.task_type {
            FabricatorTaskType::Building(build) => 
                self.builds.finish_task(creep_id, &build, success),
            FabricatorTaskType::Repairing(repair) => 
                self.repairs.finish_task(creep_id, &repair, success),
            FabricatorTaskType::UpgradingController(upgrade) => 
                self.upgrades.finish_task(creep_id, &upgrade, success),
        }
    }
}