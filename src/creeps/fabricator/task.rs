use anyhow::{Result, bail};
use derive_where::derive_where;
use screeps::{ConstructionSite, HasPosition, Position};

use crate::{check::{Check, CheckFrom}, creeps::virtual_creep::{IntentError, VirtualCreep}, domain_traits::{HasHits, ObjectId, ResolvableId}, ids::{CheckState, Checked, Unchecked}, structure::RepairableStructure};

#[derive(Debug)]
#[derive_where(Serialize, Deserialize, Clone; StructureTask<S>)]
pub enum FabricatorTask<S: CheckState = Checked> {
    Structure(StructureTask<S>),
    Upgrading
}

impl CheckFrom for FabricatorTask {
    type Unchecked = FabricatorTask<Unchecked>;
    type Err = anyhow::Error;

    fn check_from(uc: Self::Unchecked) -> Result<Self> {
        Ok(match uc {
            FabricatorTask::Structure(task) => Self::Structure(task.check()?),
            FabricatorTask::Upgrading => Self::Upgrading,
        })
    }
}

#[derive(Debug)]
#[derive_where(Serialize, Deserialize, Clone; BuildTask<S>, RepairTask<S>)]
pub enum StructureTask<S: CheckState = Checked> {
    Building(BuildTask<S>),
    Repairing(RepairTask<S>)
}

impl CheckFrom for StructureTask {
    type Unchecked = StructureTask<Unchecked>;
    type Err = anyhow::Error;

    fn check_from(us: Self::Unchecked) -> Result<Self> {
        Ok(match us {
            StructureTask::Building(id) => 
                Self::Building(id.check()?),
            StructureTask::Repairing(id) => {
                let structure: RepairableStructure = id.check()?;
                if structure.hits() == structure.hits_max() { bail!("Structure no longer needs repair") }

                Self::Repairing(structure)
            }
        })
    }
}

pub type BuildTask<S = Checked> = ObjectId<ConstructionSite, S>;
pub type RepairTask<S = Checked> = RepairableStructure::<S>;

impl StructureTask {
    pub fn creep_work(&self, creep: &mut VirtualCreep) -> anyhow::Result<u32, IntentError> {
        match self {
            StructureTask::Building(site) => 
                creep.build(site.resolve()),
            StructureTask::Repairing(structure) => 
                creep.repair(*structure),
        }
    }

    pub fn pos(&self) -> Position {
        match self {
            StructureTask::Building(id) => id.resolve().pos(),
            StructureTask::Repairing(id) => id.pos(),
        }
    }
}