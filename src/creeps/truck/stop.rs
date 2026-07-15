use std::hash::Hash;

use anyhow::{anyhow, bail};
use derive_where::derive_where;
use screeps::{Creep, HasPosition, Position, Resource, ResourceType, Ruin, Tombstone};

use crate::{check::{Check, CheckFrom}, creeps::virtual_creep::{IntentError, VirtualCreep}, domain_traits::{HasStore, HasStoreExt, ObjectId, ResolvableId, Transferable}, ids::{CheckState, Checked, Unchecked}, structure::{ConsumerStructure, ProviderStructure}};

#[derive_where(Debug, PartialEq, Eq, Hash)]
#[derive_where(Serialize, Deserialize, Clone; ObjectId<Ruin, S>, ObjectId<Resource, S>, ObjectId<Tombstone, S>, ProviderStructure<S>, ObjectId<Creep, S>)]
pub enum ProviderTruckStop<S: CheckState = Checked> {
    Ruin(ObjectId<Ruin, S>),
    Resource(ObjectId<Resource, S>),
    Tombstone(ObjectId<Tombstone, S>),
    Structure(ProviderStructure<S>),
    Creep(ObjectId<Creep, S>)
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