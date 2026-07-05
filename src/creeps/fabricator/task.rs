use anyhow::Result;
use derive_where::derive_where;
use screeps::{ConstructionSite, HasPosition, Position};

use crate::{check::{Check, CheckFrom}, colony::ColonyView, creeps::{fabricator::coordinator::FabricatorTaskHandle, virtual_creep::{IntentError, VirtualCreep}}, ids::{CheckState, Checked, Unchecked, WithId}, structure::RepairableStructure};

#[derive(Debug)]
#[derive_where(Serialize, Deserialize, Clone; BuildTask<S>, RepairTask<S>)]
pub enum FabricatorTask<S: CheckState = Checked> {
    Building(BuildTask<S>),
    Repairing(RepairTask<S>),
    UpgradingController
}

impl CheckFrom for FabricatorTask {
    type Unchecked = FabricatorTask<Unchecked>;
    type Err = anyhow::Error;

    fn check_from(us: Self::Unchecked) -> Result<Self> {
        Ok(match us {
            FabricatorTask::Building(id) => Self::Building(id.check()?),
            FabricatorTask::Repairing(id) => Self::Repairing(id.check()?),
            FabricatorTask::UpgradingController => Self::UpgradingController,
        })
    }
}

pub type BuildTask<S = Checked> = <S as CheckState>::Repr<WithId<ConstructionSite>>;
pub type RepairTask<S = Checked> = RepairableStructure::<S>;

impl FabricatorTask {
    pub fn work_range(&self) -> u32 {
        match self {
            FabricatorTask::Building(_) | FabricatorTask::Repairing(_) => 1,
            FabricatorTask::UpgradingController => 3,
        }
    }

    pub fn creep_work(&self, creep: &mut VirtualCreep, home: &ColonyView<'_>, handle: &mut FabricatorTaskHandle) -> anyhow::Result<(), IntentError> {
        match self {
            FabricatorTask::Building(site) => {
                let FabricatorTaskHandle::Collab(handle) = handle else { unreachable!() };
                handle.apply_work(creep.build((***site).clone())?);
                
                Ok(())
            },
            FabricatorTask::Repairing(structure) => {
                let FabricatorTaskHandle::Collab(handle) = handle else { unreachable!() };
                handle.apply_work(creep.repair(structure.clone())?);
                
                Ok(())
            },
            FabricatorTask::UpgradingController => 
                creep.upgrade_controller(home.controller.clone()).map(|_| ()),
        }
    }

    pub fn pos(&self, home: &ColonyView<'_>) -> Position {
        match self {
            FabricatorTask::Building(id) => id.pos(),
            FabricatorTask::Repairing(id) => id.pos(),
            FabricatorTask::UpgradingController => home.controller.pos(),
        }
    }
}