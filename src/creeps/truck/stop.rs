use std::hash::Hash;

use anyhow::{anyhow, bail};
use derive_where::derive_where;
use screeps::{Creep, HasPosition, Position, Resource, ResourceType, Ruin, Tombstone};

use crate::{check::{Check, CheckFrom}, creeps::virtual_creep::{IntentError, VirtualCreep}, domain_traits::{HasStore, HasStoreExt, Transferable}, ids::{ById, CheckState, Checked, Unchecked, WithId}, structure::{ConsumerStructure, ProviderStructure}};

#[derive_where(Debug, PartialEq, Eq, Hash)]
#[derive_where(Serialize, Deserialize, Clone; S::Repr<Ruin>, S::Repr<Resource>, S::Repr<Tombstone>, ProviderStructure<S>, S::Repr<WithId<Creep>>)]
pub enum ProviderTruckStop<S: CheckState = Checked> {
    Ruin(S::Repr<Ruin>),
    Resource(S::Repr<Resource>),
    Tombstone(S::Repr<Tombstone>),
    Structure(ProviderStructure<S>),
    Creep(S::Repr<WithId<Creep>>)
}

impl CheckFrom for ProviderTruckStop {
    type Unchecked = ProviderTruckStop<Unchecked>;
    type Err = anyhow::Error;

    fn check_from(uc: Self::Unchecked) -> Result<Self, Self::Err> {
        let checked = match uc {
            Self::Unchecked::Ruin(x) => Self::Ruin(ById(x.check()?)),
            Self::Unchecked::Resource(x) => Self::Resource(ById(x.check()?)),
            Self::Unchecked::Tombstone(x) => Self::Tombstone(ById(x.check()?)),
            Self::Unchecked::Structure(x) => Self::Structure(x.check()?),
            Self::Unchecked::Creep(x) => Self::Creep(ById(x.check()?)),
        };

        if checked.get_resource_avaliable(ResourceType::Energy) == 0 { bail!("Provider is empty"); }
        Ok(checked)
    }
}

impl ProviderTruckStop {
    pub fn pos(&self) -> Position { 
        match self {
            Self::Ruin(id) => id.pos(),
            Self::Resource(id) => id.pos(),
            Self::Tombstone(id) => id.pos(),
            Self::Structure(id) => id.pos(),
            Self::Creep(id) => id.pos(),
        }
    }

    pub fn get_resource_avaliable(&self, ty: ResourceType) -> u32 { 
        match self {
            Self::Ruin(id) => id.store().get_used_capacity(Some(ty)),
            Self::Tombstone(id) => id.store().get_used_capacity(Some(ty)),
            Self::Structure(id) => id.store().get_used_capacity(Some(ty)),
            Self::Creep(id) => id.store().get_used_capacity(Some(ty)),
            Self::Resource(id) => 
                if id.resource_type() == ty { id.amount() } else { 0 },
        }
    }

    pub fn creep_withdraw(&self, creep: &mut VirtualCreep, ty: ResourceType) -> anyhow::Result<u32, IntentError> { 
        match self {
            Self::Ruin(id) => Ok(creep.withdraw((**id).clone(), ty, None)?),
            Self::Tombstone(id) => Ok(creep.withdraw((**id).clone(), ty, None)?),
            Self::Creep(id) => Ok(creep.transfer_from(id, ty, None)?),
            Self::Structure(id) => creep.withdraw(id.clone(), ty, None),
            Self::Resource(id) => 
                if id.resource_type() == ty { 
                    Ok(creep.pickup((**id).clone())?) 
                } else { 
                    Err(anyhow!("Resource pile does not contain {ty}").into()) 
                },
        }
    }

}

#[derive(Debug, PartialEq, Eq, Hash)]
#[derive_where(Serialize, Deserialize, Clone; ConsumerStructure<S>, S::Repr<WithId<Creep>>)]
pub enum ConsumerTruckStop<S: CheckState = Checked> {
    Structure(ConsumerStructure<S>),
    Creep(S::Repr<WithId<Creep>>)
}

impl CheckFrom for ConsumerTruckStop {
    type Unchecked = ConsumerTruckStop<Unchecked>;
    type Err = anyhow::Error;

    fn check_from(us: Self::Unchecked) -> Result<Self, Self::Err> {
        let checked = match us {
            Self::Unchecked::Structure(x) => Self::Structure(x.check()?),
            Self::Unchecked::Creep(x) => Self::Creep(ById(x.check()?)),
        };

        if checked.free_capacity(Some(ResourceType::Energy)) == 0 { bail!("Consumer has no free space") }
        Ok(checked)
    }
}

impl ConsumerTruckStop {
    pub fn pos(&self) -> Position {
        match self {
            Self::Structure(id) => id.pos(),
            Self::Creep(id) => id.pos(),
        }
    }
}

impl HasStore for ConsumerTruckStop {
    fn store(&self) -> screeps::Store {
        match self {
            ConsumerTruckStop::Structure(structure) => structure.store(),
            ConsumerTruckStop::Creep(creep) => creep.store(),
        }
    }
}

impl Transferable for ConsumerTruckStop {
    fn transferable(&self) -> &dyn screeps::Transferable {
        match self {
            ConsumerTruckStop::Structure(structure) => structure.transferable(),
            ConsumerTruckStop::Creep(creep) => creep.as_ref(),
        }
    }
}