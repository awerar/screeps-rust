use anyhow::anyhow;
use derive_deref::Deref;
use derive_where::derive_where;
use enum_display::EnumDisplay;
use screeps::{ConstructionSite, Creep, HasPosition, Part, Position, ResourceType, Room, SharedCreepProperties, Structure, StructureController, StructureObject, controller_downgrade, find, game};
use serde::{Serialize, Deserialize};
use derive_alias::derive_alias;

use crate::{check::{Check, CheckFrom}, colony::{ColonyBuffer, ColonyView}, domain_traits::{EnergyStoreAccessors, Withdrawable}, ids::{WithId, Checked, Handle, CheckState, IntoWithId, Unchecked}, movement::requests::MovementRequests, statemachine::Transition, tasks::{TaskServer, prune_deserialize_taskserver}};

#[derive(Debug, Default, EnumDisplay)]
#[derive_where(Serialize, Deserialize, Clone; FabricatorTask<I>)]
pub enum FabricatorCreep<I: CheckState = Checked> {
    #[default] Idle,
    CollectingFor(FabricatorTask<I>),
    Performing(FabricatorTask<I>)
}

impl<'de> Deserialize<'de> for FabricatorCreep {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let us = FabricatorCreep::<Unchecked>::deserialize(deserializer)?;
        Ok(match us {
            FabricatorCreep::Idle => Self::Idle,
            FabricatorCreep::CollectingFor(task) => 
                task.check().map_or(FabricatorCreep::Idle, FabricatorCreep::CollectingFor),
            FabricatorCreep::Performing(task) => 
                task.check().map_or(FabricatorCreep::Idle, FabricatorCreep::Performing),
        })
    }
}

#[derive(Debug)]
#[derive_where(Serialize, Deserialize, Clone; BuildTask<I>, RepairTask<I>, UpgradeTask<I>)]
pub enum FabricatorTaskType<I: CheckState = Checked> {
    Building(BuildTask<I>),
    Repairing(RepairTask<I>),
    UpgradingController(UpgradeTask<I>)
}

#[derive(Debug)]
#[derive_where(Serialize, Deserialize, Clone; FabricatorTaskType<I>)]
pub struct FabricatorTask<I: CheckState = Checked> {
    task_type: FabricatorTaskType<I>,
    start_time: u32,
    pos: Position
}

impl CheckFrom for FabricatorTask {
    type Unchecked = FabricatorTask<Unchecked>;
    type Err = ();

    fn check_from(us: Self::Unchecked) -> Result<Self, ()> {
        Ok(Self { 
            task_type: match us.task_type {
                FabricatorTaskType::Building(id) => 
                    FabricatorTaskType::Building(id.check()?),
                FabricatorTaskType::Repairing(id) => 
                    FabricatorTaskType::Repairing(id.check()?),
                FabricatorTaskType::UpgradingController(id) => 
                    FabricatorTaskType::UpgradingController(id.check()?),
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

type BuildTask<I = Checked> = <I as CheckState>::Repr<WithId<ConstructionSite>>;
type RepairTask<I = Checked> = <I as CheckState>::Repr<Structure>;
type UpgradeTask<I = Checked> = <I as CheckState>::Repr<StructureController>;

const REPAIR_PERCENTAGE: HealthPercentage = HealthPercentage(0.75);
const EMERGENCY_REPAIR_PERCENTAGE: HealthPercentage = HealthPercentage(0.5);
const CONTROLLER_DOWNGRADE_EMERGENCY_PERCENTAGE: DowngradePercentage = DowngradePercentage(0.5);
const STORAGE_UPGRADE_CONTROLLER_THRESHOLD: StorageFillPercentage = StorageFillPercentage(0.1);

const MAX_TASK_TICKS: u32 = 100;
const GUESSED_CREEP_MOVE_TO_TASK_TICKS: u32 = 50;

impl FabricatorCreep {
    pub fn is_consumer(&self) -> bool { matches!(self, Self::CollectingFor(_) | Self::Performing(_)) }
    pub fn is_provider(&self) -> bool { matches!(self, Self::Idle) }

    pub fn update(self, creep: &WithId<Creep>, home: &ColonyView<'_>, movement: &mut MovementRequests, coordinator: &mut FabricatorCoordinator) -> anyhow::Result<Transition<Self>> {
        use Transition::*;

        match self {
            Self::Idle => {
                let task = coordinator.assign_task(creep);
                if let Some(task) = task {
                    return Ok(Continue(Self::Performing(task)))
                }

                Ok(Break(self))
            },
            Self::CollectingFor(ref task) => {
                if task.has_timed_out() || !coordinator.heartbeat_task(&creep.dumb_id(), task) { return Self::fail_task(creep, task, coordinator) }

                if creep.used_energy_capacity() > 0 {
                    return Ok(Continue(Self::Performing(task.clone())))
                }

                let Some(buffer) = &home.buffer else { return Ok(Break(self)) };
                if buffer.used_energy_capacity() == 0 { return Ok(Break(self)) }

                if movement.move_creep_to(creep, buffer.pos(), 1).in_range() {
                    creep.withdraw(buffer.withdrawable(), ResourceType::Energy, None)?;
                    return Ok(Break(Self::Performing(task.clone())))
                }
                    
                Ok(Break(self))
            },
            Self::Performing(ref task) => {
                if task.has_timed_out() || !coordinator.heartbeat_task(&creep.dumb_id(), task) { return Self::fail_task(creep, task, coordinator) }

                let creep_energy = creep.used_energy_capacity();
                if creep_energy == 0 {
                    return Ok(Continue(Self::CollectingFor(task.clone())))
                }

                if movement.move_creep_to(creep, task.pos, task.work_range()).in_range() && creep_energy > 0 {
                    task.creep_work(creep)?;
                }

                Ok(Break(self))
            }
        }
    }

    #[expect(clippy::unnecessary_wraps)]
    fn fail_task(creep: &WithId<Creep>, task: &FabricatorTask, coordinator: &mut FabricatorCoordinator) -> anyhow::Result<Transition<Self>> {
        coordinator.finish_task(&creep.dumb_id(), task, false);
        Ok(Transition::Continue(FabricatorCreep::Idle))
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
                Ok(creep.build(site)?),
            FabricatorTaskType::Repairing(structure) => {
                let structure_object = StructureObject::from(structure.as_ref().clone());
                let repairable = structure_object.as_repairable().ok_or(anyhow!("Structure is not repairable"))?;
                Ok(creep.repair(repairable)?)
            },
            FabricatorTaskType::UpgradingController(controller) => 
                Ok(creep.upgrade_controller(controller)?),
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
                    structure.as_structure().clone().into_checked(), 
                    repairable.hits_max() - repairable.hits(),
                    (structure.pos(), 
                    HealthPercentage(repairable.hits() as f32 / repairable.hits_max() as f32))
                ))
            })
        );

        self.builds.set_tasks(room.find(find::MY_CONSTRUCTION_SITES, None).into_iter()
            .map(|site| {
                (
                    site.clone().with_id().unwrap(),
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
            controller.into_checked(), 
            u32::MAX,
            (DowngradePercentage(downgrade_percentage),
            storage_fill_percentage.map(StorageFillPercentage))
        )]);
    }

    fn assign_task(&mut self, creep: &WithId<Creep>) -> Option<FabricatorTask> {
        self.assign_emergency_upgrade(creep).map(FabricatorTaskType::UpgradingController)
            .or_else(|| self.assign_repair(creep).map(FabricatorTaskType::Repairing))
            .or_else(|| self.assign_build(creep).map(FabricatorTaskType::Building))
            .or_else(|| self.assign_upgrade(creep).map(FabricatorTaskType::UpgradingController))
            .map(FabricatorTask::new)
    }

    fn assign_repair(&mut self, creep: &WithId<Creep>) -> Option<RepairTask> {
        let contribution = get_creep_work_count(creep) * 100;
        self.repairs.assign_task(creep.dumb_id(), contribution, |tasks| {
            let emergency_repair = tasks.clone().into_iter()
                .filter(|(_, _, (_, percentage))| *percentage <= EMERGENCY_REPAIR_PERCENTAGE)
                .min_by(|(_, _, (_, p1)), (_, _, (_, p2))| p1.total_cmp(p2));
            if emergency_repair.is_some() { return emergency_repair }

            tasks.into_iter()
                .filter(|(_, _, (_, percentage))| *percentage <= REPAIR_PERCENTAGE)
                .min_by_key(|(_, _, (pos, _))| creep.pos().get_range_to(*pos))
        })
    }

    fn assign_build(&mut self, creep: &WithId<Creep>) -> Option<BuildTask> {
        let contribution = get_creep_work_count(creep) * 5;
        self.builds.assign_task(creep.dumb_id(), contribution, |tasks| {
            tasks.into_iter()
                .min_by_key(|(_, _, pos)| creep.pos().get_range_to(**pos))
        })
    }

    fn assign_emergency_upgrade(&mut self, creep: &WithId<Creep>) -> Option<UpgradeTask> {
        let contribution = get_creep_work_count(creep) * 2;
        self.upgrades.assign_task(creep.dumb_id(), contribution, |tasks| {
            tasks.into_iter()
                .find(|(_, _, (percentage, _))| *percentage >= CONTROLLER_DOWNGRADE_EMERGENCY_PERCENTAGE)
        })
    }

    fn assign_upgrade(&mut self, creep: &WithId<Creep>) -> Option<UpgradeTask> {
        let contribution = get_creep_work_count(creep) * 2;
        self.upgrades.assign_task(creep.dumb_id(), contribution, |tasks| {
            tasks.into_iter()
                .find(|(_, _, (_, percentage))| 
                    percentage.is_none_or(|percentage| percentage >= STORAGE_UPGRADE_CONTROLLER_THRESHOLD))
        })
    }

    fn heartbeat_task(&mut self, creep: &Handle<WithId<Creep>>, task: &FabricatorTask) -> bool {
        match &task.task_type {
            FabricatorTaskType::Building(build) => 
                self.builds.heartbeat_task(creep, build),
            FabricatorTaskType::Repairing(repair) => 
                self.repairs.heartbeat_task(creep, repair),
            FabricatorTaskType::UpgradingController(upgrade) => 
                self.upgrades.heartbeat_task(creep, upgrade),
        }
    }

    fn finish_task(&mut self, creep: &Handle<WithId<Creep>>, task: &FabricatorTask, success: bool) {
        match &task.task_type {
            FabricatorTaskType::Building(build) => 
                self.builds.finish_task(creep, build, success),
            FabricatorTaskType::Repairing(repair) => 
                self.repairs.finish_task(creep, repair, success),
            FabricatorTaskType::UpgradingController(upgrade) => 
                self.upgrades.finish_task(creep, upgrade, success),
        }
    }
}