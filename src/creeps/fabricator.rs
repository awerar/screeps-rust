use anyhow::anyhow;
use derive_deref::Deref;
use enum_display::EnumDisplay;
use screeps::{ConstructionSite, Creep, HasPosition, MaybeHasId, Part, Position, ResourceType, Room, SharedCreepProperties, Structure, StructureController, StructureObject, controller_downgrade, find, game};
use serde::{Serialize, Deserialize};
use derive_alias::derive_alias;

use crate::{colony::{ColonyBuffer, ColonyView}, messages::{CreepMessage, Messages, TruckMessage}, movement::Movement, safeid::{DO, GetSafeID, IDKind, SafeID, SafeIDs, TryFromUnsafe, TryGetSafeID, TryMakeSafe, UnsafeIDs}, statemachine::{StateMachine, Transition}, tasks::{TaskServer, prune_deserialize_taskserver}};

#[derive(Serialize, Deserialize, Debug, Clone, Default, EnumDisplay)]
#[serde(bound(deserialize = "FabricatorTask<I> : DO, FabricatorTask<I> : DO"))]
pub enum FabricatorCreep<I: IDKind = SafeIDs> {
    #[default] Idle,
    CollectingFor(FabricatorTask<I>),
    Performing(FabricatorTask<I>)
}

impl<'de> Deserialize<'de> for FabricatorCreep {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let us = FabricatorCreep::<UnsafeIDs>::deserialize(deserializer)?;
        Ok(match us {
            FabricatorCreep::Idle => Self::Idle,
            FabricatorCreep::CollectingFor(task) => 
                task.try_make_safe().map(FabricatorCreep::CollectingFor).unwrap_or(FabricatorCreep::Idle),
            FabricatorCreep::Performing(task) => 
                task.try_make_safe().map(FabricatorCreep::Performing).unwrap_or(FabricatorCreep::Idle),
        })
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(bound(deserialize = "BuildTask<I> : DO, RepairTask<I> : DO, UpgradeTask<I> : DO"))]
pub enum FabricatorTaskType<I: IDKind = SafeIDs> {
    Building(BuildTask<I>),
    Repairing(RepairTask<I>),
    UpgradingController(UpgradeTask<I>)
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(bound(deserialize = "FabricatorTaskType<I> : DO"))]
pub struct FabricatorTask<I: IDKind = SafeIDs> {
    task_type: FabricatorTaskType<I>,
    start_time: u32,
    pos: Position
}

impl TryFromUnsafe for FabricatorTask {
    type Unsafe = FabricatorTask<UnsafeIDs>;

    fn try_from_unsafe(us: Self::Unsafe) -> Option<Self> {
        Some(Self { 
            task_type: match us.task_type {
                FabricatorTaskType::Building(id) => 
                    FabricatorTaskType::Building(id.try_make_safe()?),
                FabricatorTaskType::Repairing(id) => 
                    FabricatorTaskType::Repairing(id.try_make_safe()?),
                FabricatorTaskType::UpgradingController(id) => 
                    FabricatorTaskType::UpgradingController(id.try_make_safe()?),
            }, 
            start_time: us.start_time, 
            pos: us.pos 
        })
    }
}

derive_alias! {
    derive_percentage => #[derive(Deref, Clone, Copy, Serialize, Deserialize, PartialEq, PartialOrd)]
}

derive_percentage! { struct HealthPercentage(f32); }
derive_percentage! { struct DowngradePercentage(f32); }
derive_percentage! { struct StorageFillPercentage(f32); }

type BuildTask<I = SafeIDs> = <I as IDKind>::ID<ConstructionSite>;
type RepairTask<I = SafeIDs> = <I as IDKind>::ID<Structure>;
type UpgradeTask<I = SafeIDs> = <I as IDKind>::ID<StructureController>;

const REPAIR_PERCENTAGE: HealthPercentage = HealthPercentage(0.75);
const EMERGENCY_REPAIR_PERCENTAGE: HealthPercentage = HealthPercentage(0.5);
const CONTROLLER_DOWNGRADE_EMERGENCY_PERCENTAGE: DowngradePercentage = DowngradePercentage(0.5);
const STORAGE_UPGRADE_CONTROLLER_THRESHOLD: StorageFillPercentage = StorageFillPercentage(0.1);

const MAX_TASK_TICKS: u32 = 100;
const GUESSED_CREEP_MOVE_TO_TASK_TICKS: u32 = 50;

type Args<'a> = (ColonyView<'a>, &'a mut Movement, &'a mut FabricatorCoordinator, &'a mut Messages);
impl StateMachine<SafeID<Creep>, Args<'_>> for FabricatorCreep {
    fn update(self, creep: &SafeID<Creep>, args: &mut Args<'_>) -> anyhow::Result<Transition<Self>> {
        use Transition::*;

        let (home, movement, coordinator, messages) = args;

        match &self {
            Self::Idle => {
                let task = coordinator.assign_task(creep);
                if let Some(task) = task {
                    return Ok(Continue(Self::Performing(task)))
                }

                messages.trucks.send(TruckMessage::Provider(creep.clone(), home.name));
                Ok(Break(self))
            },
            Self::CollectingFor(task) => {
                if task.has_timed_out() || !coordinator.heartbeat_task(creep, task) {
                    coordinator.finish_task(creep, task, false);
                    return Ok(Break(Self::Idle))
                }

                if messages.creep(creep).read(CreepMessage::TruckTarget) 
                    || creep.store().get_used_capacity(Some(ResourceType::Energy)) > 0 {
                        return Ok(Continue(Self::Performing(task.clone())))
                    }

                messages.trucks.send(TruckMessage::Consumer(creep.clone(), home.name));

                let Some(buffer) = &home.buffer else {
                    return Ok(Break(Self::CollectingFor(task.clone())))
                };

                if creep.pos().is_near_to(buffer.pos()) {
                    if buffer.store().get_used_capacity(Some(ResourceType::Energy)) > 0 {
                        creep.withdraw(buffer.withdrawable(), ResourceType::Energy, None)?;
                        Ok(Break(Self::Performing(task.clone())))
                    } else { Ok(Break(self)) }
                } else {
                    movement.smart_move_creep_to(creep, buffer.pos()).ok();
                    Ok(Break(self))
                }
            },
            Self::Performing(task) => {
                if task.has_timed_out() || !coordinator.heartbeat_task(creep, task) {
                    coordinator.finish_task(creep, task, false);
                    return Ok(Continue(Self::Idle))
                }

                let creep_energy = creep.store().get_used_capacity(Some(ResourceType::Energy));
                if creep_energy == 0 && !messages.creep(creep).read(CreepMessage::TruckTarget) {
                    return Ok(Continue(Self::CollectingFor(task.clone())))
                }

                messages.trucks.send(TruckMessage::Consumer(creep.clone(), home.name));

                if !creep.pos().is_near_to(task.pos) {
                    movement.smart_move_creep_to(creep, task.pos).ok();
                }

                if creep_energy > 0 && creep.pos().get_range_to(task.pos) <= task.work_range() {
                    task.creep_work(creep)?;
                }

                Ok(Break(self))
            }
        }
    }
}

impl FabricatorTask {
    fn new(task_type: FabricatorTaskType) -> Self {
        Self {
            start_time: game::time(),
            pos: task_type.pos(),
            task_type,
        }
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

    fn creep_work(&self, creep: &Creep) -> anyhow::Result<()> {
        match &self.task_type {
            FabricatorTaskType::Building(site) => 
                Ok(creep.build(&site)?),
            FabricatorTaskType::Repairing(structure) => {
                let structure_object = StructureObject::from(structure.as_ref().clone());
                let repairable = structure_object.as_repairable().ok_or(anyhow!("Structure is not repairable"))?;
                Ok(creep.repair(repairable)?)
            },
            FabricatorTaskType::UpgradingController(controller) => 
                Ok(creep.upgrade_controller(&controller)?),
        }
    }
}

impl FabricatorTaskType {
    fn pos(&self) -> Position {
        match self {
            FabricatorTaskType::Building(id) => id.pos(),
            FabricatorTaskType::Repairing(id) => id.pos(),
            FabricatorTaskType::UpgradingController(id) => id.pos(),
        }
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
    #[serde(deserialize_with = "prune_deserialize_taskserver")] 
    repairs: TaskServer<RepairTask, (Position, HealthPercentage)>,
    #[serde(deserialize_with = "prune_deserialize_taskserver")] 
    builds: TaskServer<BuildTask, Position>,
    #[serde(deserialize_with = "prune_deserialize_taskserver")] 
    upgrades: TaskServer<UpgradeTask, (DowngradePercentage, Option<StorageFillPercentage>)>
}

impl FabricatorCoordinator {
    pub fn update(&mut self, room: &Room, buffer: Option<ColonyBuffer>) {
        self.repairs.set_tasks(room.find(find::STRUCTURES, None).into_iter()
            .filter_map(|structure| {
                let repairable = structure.as_repairable()?;
                Some((
                    structure.as_structure().safe_id(), 
                    repairable.hits_max() - repairable.hits(),
                    (structure.pos(), 
                    HealthPercentage(repairable.hits() as f32 / repairable.hits_max() as f32))
                ))
            })
        );

        self.builds.set_tasks(room.find(find::MY_CONSTRUCTION_SITES, None).into_iter()
            .map(|site| {
                (
                    site.try_safe_id().unwrap(),
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
            controller.safe_id(), 
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
            .map(FabricatorTask::new)
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
        match &task.task_type {
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

        match &task.task_type {
            FabricatorTaskType::Building(build) => 
                self.builds.finish_task(creep_id, &build, success),
            FabricatorTaskType::Repairing(repair) => 
                self.repairs.finish_task(creep_id, &repair, success),
            FabricatorTaskType::UpgradingController(upgrade) => 
                self.upgrades.finish_task(creep_id, &upgrade, success),
        }
    }
}