use anyhow::Result;
use derive_where::derive_where;
use screeps::{ConstructionSite, HasPosition, Position, StructureController};

use crate::{check::{Check, CheckFrom}, creeps::virtual_creep::{IntentError, VirtualCreep}, ids::{CheckState, Checked, Unchecked, WithId}, structure::RepairableStructure};

#[derive(Debug)]
#[derive_where(Serialize, Deserialize, Clone; BuildTask<S>, RepairTask<S>, UpgradeTask<S>)]
pub enum FabricatorTask<S: CheckState = Checked> {
    Building(BuildTask<S>),
    Repairing(RepairTask<S>),
    UpgradingController(UpgradeTask<S>)
}

impl CheckFrom for FabricatorTask {
    type Unchecked = FabricatorTask<Unchecked>;
    type Err = anyhow::Error;

    fn check_from(us: Self::Unchecked) -> Result<Self> {
        Ok(match us {
            FabricatorTask::Building(id) => Self::Building(id.check()?),
            FabricatorTask::Repairing(id) => Self::Repairing(id.check()?),
            FabricatorTask::UpgradingController(id) => Self::UpgradingController(id.check()?),
        })
    }
}

pub type BuildTask<S = Checked> = <S as CheckState>::Repr<WithId<ConstructionSite>>;
pub type RepairTask<S = Checked> = RepairableStructure::<S>;
pub type UpgradeTask<S = Checked> = <S as CheckState>::Repr<StructureController>;

impl FabricatorTask {
    pub fn work_range(&self) -> u32 {
        match self {
            FabricatorTask::Building(_) | FabricatorTask::Repairing(_) => 1,
            FabricatorTask::UpgradingController(_) => 3,
        }
    }

    pub fn creep_work(&self, creep: &mut VirtualCreep) -> anyhow::Result<u32, IntentError> {
        match self {
            FabricatorTask::Building(site) => 
                creep.build((***site).clone()),
            FabricatorTask::Repairing(structure) => {
                creep.repair(structure.clone())
            },
            FabricatorTask::UpgradingController(controller) => 
                creep.upgrade_controller((**controller).clone()),
        }
    }

    pub fn pos(&self) -> Position {
        match self {
            FabricatorTask::Building(id) => id.pos(),
            FabricatorTask::Repairing(id) => id.pos(),
            FabricatorTask::UpgradingController(id) => id.pos(),
        }
    }
}