use anyhow::{anyhow, Result};
use derive_where::derive_where;
use screeps::{ConstructionSite, HasPosition, Position, Structure, StructureController, StructureObject, game};

use crate::{check::{Check, CheckFrom}, creeps::virtual_creep::{IntentError, VirtualCreep}, ids::{CheckState, Checked, Unchecked, WithId}};

#[derive(Debug)]
#[derive_where(Serialize, Deserialize, Clone; BuildTask<S>, RepairTask<S>, UpgradeTask<S>)]
pub enum FabricatorTaskType<S: CheckState = Checked> {
    Building(BuildTask<S>),
    Repairing(RepairTask<S>),
    UpgradingController(UpgradeTask<S>)
}

#[derive(Debug)]
#[derive_where(Serialize, Deserialize, Clone; FabricatorTaskType<S>)]
pub struct FabricatorTask<S: CheckState = Checked> {
    task_type: FabricatorTaskType<S>,
    start_time: u32,
    pos: Position
}

impl CheckFrom for FabricatorTask {
    type Unchecked = FabricatorTask<Unchecked>;
    type Err = anyhow::Error;

    fn check_from(us: Self::Unchecked) -> Result<Self> {
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

pub type BuildTask<S = Checked> = <S as CheckState>::Repr<WithId<ConstructionSite>>;
pub type RepairTask<S = Checked> = <S as CheckState>::Repr<Structure>;
pub type UpgradeTask<S = Checked> = <S as CheckState>::Repr<StructureController>;

impl FabricatorTask {
   pub fn new(task_type: FabricatorTaskType) -> Self {
        Self {
            start_time: game::time(),
            pos: task_type.pos(),
            task_type,
        }
    }

    // TODO: Timeout on check instead
    pub fn has_timed_out(&self) -> bool {
        game::time() >= self.start_time + super::MAX_TASK_TICKS
    }

    pub fn work_range(&self) -> u32 {
        match self.task_type {
            FabricatorTaskType::Building(_) | FabricatorTaskType::Repairing(_) => 1,
            FabricatorTaskType::UpgradingController(_) => 3,
        }
    }

    pub fn creep_work(&self, creep: &mut VirtualCreep) -> anyhow::Result<u32, IntentError> {
        match &self.task_type {
            FabricatorTaskType::Building(site) => 
                creep.build(***site),
            FabricatorTaskType::Repairing(structure) => {
                let structure_object = StructureObject::from((**structure).clone());
                let repairable = structure_object.as_repairable().ok_or(anyhow!("Structure is not repairable"))?;
                creep.repair(repairable)
            },
            FabricatorTaskType::UpgradingController(controller) => 
                creep.upgrade_controller(**controller),
        }
    }

    pub fn task_type(&self) -> &FabricatorTaskType {
        &self.task_type
    }

    pub fn pos(&self) -> Position {
        self.pos
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