use anyhow::anyhow;
use derive_deref::Deref;
use enum_display::EnumDisplay;
use screeps::{ConstructionSite, Creep, HasPosition, MaybeHasId, Part, Position, ResourceType, Room, SharedCreepProperties, Structure, StructureController, StructureObject, controller_downgrade, find, game};
use serde::{Serialize, Deserialize};
use derive_alias::derive_alias;

use crate::{colony::{ColonyBuffer, ColonyView}, id::{IDMaybeResolvable, IDMode, IDResolvable, Resolved, Unresolved}, messages::{CreepMessage, Messages, TruckMessage}, movement::Movement, statemachine::{StateMachine, Transition}, tasks::TaskServer};

#[derive(Serialize, Deserialize, Debug, Clone, Default, EnumDisplay)]
pub enum FabricatorCreep<M: IDMode> {
    #[default] Idle,
    CollectingFor(FabricatorTask<M>),
    Performing(FabricatorTask<M>)
}

impl IDResolvable for FabricatorCreep<Unresolved> {
    type Target = FabricatorCreep<Resolved>;

    fn id_resolve(self) -> Self::Target {
        match self {
            Self::Idle => FabricatorCreep::Idle,
            Self::CollectingFor(task) => 
                task.try_id_resolve().map(FabricatorCreep::CollectingFor).unwrap_or(FabricatorCreep::Idle),
            Self::Performing(task) => 
                task.try_id_resolve().map(FabricatorCreep::Performing).unwrap_or(FabricatorCreep::Idle),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FabricatorTask<M: IDMode> {
    task_type: FabricatorTaskType<M>,
    start_time: u32,
    pos: Position
}

impl IDMaybeResolvable for FabricatorTask<Unresolved> {
    type Target = FabricatorTask<Resolved>;

    fn try_id_resolve(self) -> Option<Self::Target> {
        Some(FabricatorTask {
            task_type: self.task_type.try_id_resolve()?,
            start_time: self.start_time,
            pos: self.pos,
        })
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum FabricatorTaskType<M: IDMode> {
    Building(BuildTask<M>),
    Repairing(RepairTask<M>),
    UpgradingController(UpgradeTask<M>)
}

impl IDMaybeResolvable for FabricatorTaskType<Unresolved> {
    type Target = FabricatorTaskType<Resolved>;

    fn try_id_resolve(self) -> Option<Self::Target> {
        Some(match self {
            Self::Building(task) => FabricatorTaskType::Building(task.try_id_resolve()?),
            Self::Repairing(task) => FabricatorTaskType::Repairing(task.try_id_resolve()?),
            Self::UpgradingController(task) => FabricatorTaskType::UpgradingController(task.try_id_resolve()?),
        })
    }
}

derive_alias! {
    derive_percentage => #[derive(Deref, Clone, Copy, Serialize, Deserialize, PartialEq, PartialOrd)]
}

derive_percentage! { struct HealthPercentage(f32); }
derive_percentage! { struct DowngradePercentage(f32); }
derive_percentage! { struct StorageFillPercentage(f32); }

pub type BuildTask<M: IDMode> = M::Wrap<ConstructionSite>;
pub type RepairTask<M: IDMode> = M::Wrap<Structure>;
pub type UpgradeTask<M: IDMode> = M::Wrap<StructureController>;

const REPAIR_PERCENTAGE: HealthPercentage = HealthPercentage(0.75);
const EMERGENCY_REPAIR_PERCENTAGE: HealthPercentage = HealthPercentage(0.5);
const CONTROLLER_DOWNGRADE_EMERGENCY_PERCENTAGE: DowngradePercentage = DowngradePercentage(0.5);
const STORAGE_UPGRADE_CONTROLLER_THRESHOLD: StorageFillPercentage = StorageFillPercentage(0.1);

const MAX_TASK_TICKS: u32 = 100;
const GUESSED_CREEP_MOVE_TO_TASK_TICKS: u32 = 50;

pub type Args<'a> = (ColonyView<'a>, &'a mut Movement<Resolved>, &'a mut FabricatorCoordinator<Resolved>, &'a mut Messages<Resolved>);
impl StateMachine<Creep, Args<'_>> for FabricatorCreep<Resolved> {
    fn update(self, creep: &Creep, args: &mut Args<'_>) -> anyhow::Result<Transition<Self>> {
        use Transition::*;

        let (home, movement, coordinator, messages) = args;

        match &self {
            Self::Idle => {
                let task = coordinator.assign_task(creep);
                if let Some(task) = task {
                    return Ok(Continue(Self::Performing(task)))
                }

                messages.trucks.send(TruckMessage::Provider(creep.into(), home.name));
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

                messages.trucks.send(TruckMessage::Consumer(creep.into(), home.name));

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

                messages.trucks.send(TruckMessage::Consumer(creep.into(), home.name));

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

impl FabricatorTask<Resolved> {
    fn new(task_type: FabricatorTaskType<Resolved>) -> Self {
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
                let structure_object = StructureObject::from(structure.cloned());
                let repairable = structure_object.as_repairable().ok_or(anyhow!("Structure is not repairable"))?;
                Ok(creep.repair(repairable)?)
            },
            FabricatorTaskType::UpgradingController(controller) => 
                Ok(creep.upgrade_controller(&controller)?),
        }
    }
}

impl FabricatorTaskType<Resolved> {
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
pub struct FabricatorCoordinator<M: IDMode + 'static> {
    repairs: TaskServer<RepairTask<M>, (Position, HealthPercentage)>,
    builds: TaskServer<BuildTask<M>, Position>,
    upgrades: TaskServer<UpgradeTask<M>, (DowngradePercentage, Option<StorageFillPercentage>)>
}

impl IDResolvable for FabricatorCoordinator<Unresolved> {
    type Target = FabricatorCoordinator<Resolved>;

    fn id_resolve(self) -> Self::Target {
        FabricatorCoordinator {
            repairs: self.repairs.id_resolve(),
            builds: self.builds.id_resolve(),
            upgrades: self.upgrades.id_resolve(),
        }
    }
}

impl FabricatorCoordinator<Resolved> {
    pub fn update(&mut self, room: &Room, buffer: Option<ColonyBuffer>) {
        self.repairs.set_tasks(room.find(find::STRUCTURES, None).into_iter()
            .filter_map(|structure| {
                let repairable = structure.as_repairable()?;
                Some((
                    structure.as_structure().into(), 
                    repairable.hits_max() - repairable.hits(),
                    (structure.pos(), 
                    HealthPercentage(repairable.hits() as f32 / repairable.hits_max() as f32))
                ))
            })
        );

        self.builds.set_tasks(room.find(find::MY_CONSTRUCTION_SITES, None).into_iter()
            .map(|site| {
                let amount = site.progress_total() - site.progress();
                let pos = site.pos();
                (site.into(), amount, pos)
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
            controller.into(), 
            u32::MAX,
            (DowngradePercentage(downgrade_percentage),
            storage_fill_percentage.map(StorageFillPercentage))
        )]);
    }

    fn assign_task(&mut self, creep: &Creep) -> Option<FabricatorTask<Resolved>> {
        self.assign_emergency_upgrade(creep).map(FabricatorTaskType::UpgradingController)
            .or_else(|| self.assign_repair(creep).map(FabricatorTaskType::Repairing))
            .or_else(|| self.assign_build(creep).map(FabricatorTaskType::Building))
            .or_else(|| self.assign_upgrade(creep).map(FabricatorTaskType::UpgradingController))
            .map(FabricatorTask::new)
    }

    fn assign_repair(&mut self, creep: &Creep) -> Option<RepairTask<Resolved>> {
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

    fn assign_build(&mut self, creep: &Creep) -> Option<BuildTask<Resolved>> {
        let contribution = get_creep_work_count(creep) * 5;
        self.builds.assign_task(creep, contribution, |tasks| {
            tasks.into_iter()
                .min_by_key(|(_, _, pos)| creep.pos().get_range_to(**pos))
        })
    }

    fn assign_emergency_upgrade(&mut self, creep: &Creep) -> Option<UpgradeTask<Resolved>> {
        let contribution = get_creep_work_count(creep) * 2;
        self.upgrades.assign_task(creep, contribution, |tasks| {
            tasks.into_iter()
                .find(|(_, _, (percentage, _))| *percentage >= CONTROLLER_DOWNGRADE_EMERGENCY_PERCENTAGE)
        })
    }

    fn assign_upgrade(&mut self, creep: &Creep) -> Option<UpgradeTask<Resolved>> {
        let contribution = get_creep_work_count(creep) * 2;
        self.upgrades.assign_task(creep, contribution, |tasks| {
            tasks.into_iter()
                .find(|(_, _, (_, percentage))| 
                    percentage.is_none_or(|percentage| percentage >= STORAGE_UPGRADE_CONTROLLER_THRESHOLD))
        })
    }

    fn heartbeat_task(&mut self, creep: &Creep, task: &FabricatorTask<Resolved>) -> bool {
        match &task.task_type {
            FabricatorTaskType::Building(build) => 
                self.builds.heartbeat_task(creep, &build),
            FabricatorTaskType::Repairing(repair) => 
                self.repairs.heartbeat_task(creep, &repair),
            FabricatorTaskType::UpgradingController(upgrade) => 
                self.upgrades.heartbeat_task(creep, &upgrade),
        }
    }

    fn finish_task(&mut self, creep: &Creep, task: &FabricatorTask<Resolved>, success: bool) {
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