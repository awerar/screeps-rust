use std::hash::Hash;

use anyhow::{anyhow, bail};
use derive_where::derive_where;
use screeps::{Creep, HasPosition, Position, Resource, ResourceType, Ruin, Tombstone};

use crate::{check::{Check, CheckFrom}, creeps::virtual_creep::{IntentError, VirtualCreep}, domain_traits::{CreepId, HasStore, HasStoreExt, ObjectId, ResolvableId, Transferable}, ids::{CheckState, Checked, Unchecked}, structure::{ConsumerStructure, ProviderStructure}};

#[derive_where(Debug, PartialEq, Eq, Hash)]
#[derive_where(Serialize, Deserialize, Clone; ObjectId<Ruin, S>, ObjectId<Resource, S>, ObjectId<Tombstone, S>, ProviderStructure<S>, ObjectId<Creep, S>)]
pub enum ProviderTruckStop<S: CheckState = Checked> {
    Ruin(ObjectId<Ruin, S>),
    Resource(ObjectId<Resource, S>),
    Tombstone(ObjectId<Tombstone, S>),
    Structure(ProviderStructure<S>),
    Creep(CreepId<S>)
}

impl CheckFrom for ProviderTruckStop {
    type Unchecked = ProviderTruckStop<Unchecked>;
    type Err = anyhow::Error;

    fn check_from(uc: Self::Unchecked) -> Result<Self, Self::Err> {
        let checked = match uc {
            Self::Unchecked::Ruin(x) => Self::Ruin(x.check()?),
            Self::Unchecked::Resource(x) => Self::Resource(x.check()?),
            Self::Unchecked::Tombstone(x) => Self::Tombstone(x.check()?),
            Self::Unchecked::Structure(x) => Self::Structure(x.check()?),
            Self::Unchecked::Creep(x) => Self::Creep(x.check()?),
        };

        if checked.get_resource_avaliable(ResourceType::Energy) == 0 { bail!("Provider is empty"); }
        Ok(checked)
    }
}

impl ProviderTruckStop {
    pub fn pos(&self) -> Position { 
        match self {
            Self::Ruin(id) => id.resolve().pos(),
            Self::Resource(id) => id.resolve().pos(),
            Self::Tombstone(id) => id.resolve().pos(),
            Self::Structure(id) => id.pos(),
            Self::Creep(id) => id.resolve().pos(),
        }
    }

    pub fn get_resource_avaliable(&self, ty: ResourceType) -> u32 { 
        match self {
            Self::Ruin(id) => id.resolve().store().get_used_capacity(Some(ty)),
            Self::Tombstone(id) => id.resolve().store().get_used_capacity(Some(ty)),
            Self::Structure(id) => id.store().get_used_capacity(Some(ty)),
            Self::Creep(id) => id.resolve().store().get_used_capacity(Some(ty)),
            Self::Resource(id) => 
                if id.resolve().resource_type() == ty { id.resolve().amount() } else { 0 },
        }
    }

    pub fn creep_withdraw(&self, creep: &mut VirtualCreep, ty: ResourceType) -> anyhow::Result<u32, IntentError> { 
        match self {
            Self::Ruin(id) => Ok(creep.withdraw(id.resolve(), ty, None)?),
            Self::Tombstone(id) => Ok(creep.withdraw(id.resolve(), ty, None)?),
            Self::Creep(id) => Ok(creep.transfer_from(&id.resolve(), ty, None)?),
            Self::Structure(id) => creep.withdraw(*id, ty, None),
            Self::Resource(id) => 
                if id.resolve().resource_type() == ty { 
                    Ok(creep.pickup(id.resolve())?) 
                } else { 
                    Err(anyhow!("Resource pile does not contain {ty}").into()) 
                },
        }
    }

}

#[derive(Debug, PartialEq, Eq, Hash)]
#[derive_where(Serialize, Deserialize, Clone; ConsumerStructure<S>, ObjectId<Creep, S>)]
pub enum ConsumerTruckStop<S: CheckState = Checked> {
    Structure(ConsumerStructure<S>),
    Creep(CreepId<S>)
}

impl CheckFrom for ConsumerTruckStop {
    type Unchecked = ConsumerTruckStop<Unchecked>;
    type Err = anyhow::Error;

    fn check_from(us: Self::Unchecked) -> Result<Self, Self::Err> {
        let checked = match us {
            Self::Unchecked::Structure(x) => Self::Structure(x.check()?),
            Self::Unchecked::Creep(x) => Self::Creep(x.check()?),
        };

        if checked.free_capacity(Some(ResourceType::Energy)) == 0 { bail!("Consumer has no free space") }
        Ok(checked)
    }
}

impl ConsumerTruckStop {
    pub fn pos(&self) -> Position {
        match self {
            Self::Structure(id) => id.pos(),
            Self::Creep(id) => id.resolve().pos(),
        }
    }
}

impl HasStore for ConsumerTruckStop {
    fn store(&self) -> screeps::Store {
        match self {
            ConsumerTruckStop::Structure(structure) => structure.store(),
            ConsumerTruckStop::Creep(creep) => creep.resolve().store(),
        }
    }
}

impl Transferable for ConsumerTruckStop {
    fn transfer_from(&self, creep: &Creep, ty: ResourceType, amount: Option<u32>) -> Result<(), screeps::action_error_codes::TransferErrorCode> {
        match self {
            ConsumerTruckStop::Structure(structure) => structure.transfer_from(creep, ty, amount),
            ConsumerTruckStop::Creep(id) => id.resolve().transfer_from(creep, ty, amount),
        }
    }
}