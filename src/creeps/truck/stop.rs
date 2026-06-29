use std::hash::Hash;

use anyhow::anyhow;
use derive_where::derive_where;
use screeps::{Creep, HasPosition, Position, Resource, ResourceType, Ruin, Tombstone};

use crate::{check::{Check, CheckFrom}, creeps::{truck::stop::safe_structure::{ConsumerStructure, ProviderStructure}, virtual_creep::{IntentError, VirtualCreep}}, domain_traits::{HasStore, Transferable}, ids::{ById, CheckState, Checked, Unchecked, WithId}};

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

    fn check_from(us: Self::Unchecked) -> Result<Self, Self::Err> {
        Ok(match us {
            Self::Unchecked::Ruin(x) => Self::Ruin(ById(x.check()?)),
            Self::Unchecked::Resource(x) => Self::Resource(ById(x.check()?)),
            Self::Unchecked::Tombstone(x) => Self::Tombstone(ById(x.check()?)),
            Self::Unchecked::Structure(x) => Self::Structure(x.check()?),
            Self::Unchecked::Creep(x) => Self::Creep(ById(x.check()?)),
        })
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

    pub fn creep_withdraw(&self, creep: &mut VirtualCreep, ty: ResourceType) -> anyhow::Result<(), IntentError> { 
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
        Ok(match us {
            Self::Unchecked::Structure(x) => Self::Structure(x.check()?),
            Self::Unchecked::Creep(x) => Self::Creep(ById(x.check()?)),
        })
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

pub mod safe_structure {
#   ![allow(clippy::missing_panics_doc)]

    use std::marker::PhantomData;

    use derive_where::derive_where;
use screeps::{HasPosition, Position, Store, Structure};
    use serde::{Deserialize, Serialize};

    use crate::{check::{Check, CheckFrom}, domain_traits::{HasStore, Transferable, Withdrawable, screeps_objects::IdResolutionError}, ids::{CheckState, Checked, Unchecked}, utils::EasyStructure};

    #[derive_where(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash; EasyStructure<S>)]
    pub struct SafeStructure<T, S: CheckState = Checked>(EasyStructure<S>, PhantomData<T>);

    impl<T> CheckFrom for SafeStructure<T> {
        type Unchecked = SafeStructure<T, Unchecked>;
        type Err = IdResolutionError<Structure>;
    
        fn check_from(us: Self::Unchecked) -> Result<Self, Self::Err> {
            Ok(SafeStructure(us.0.check()?, PhantomData))
        }
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)] pub struct Consumer;
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)] pub struct Provider;

    pub type ConsumerStructure<S = Checked> = SafeStructure<Consumer, S>;
    pub type ProviderStructure<S = Checked> = SafeStructure<Provider, S>;

    impl<T> SafeStructure<T> {
        pub fn pos(&self) -> Position { self.0.pos() }
    }

    impl<T> HasStore for SafeStructure<T> {
        fn store(&self) -> Store { self.0.structure_object().as_has_store().unwrap().store() }
    }

    pub trait ConsumerStructureReqs = Into<Structure> + HasStore + Transferable;
    impl ConsumerStructure {
        pub fn new<S: ConsumerStructureReqs>(structure: S) -> Self {
            Self(EasyStructure::new(structure.into()), PhantomData)
        }
    }

    impl Transferable for ConsumerStructure {
        fn transferable(&self) -> &dyn screeps::Transferable { self.0.structure_object().as_transferable().unwrap() }
    }

    pub trait ProviderStructureReqs = Into<Structure> + HasStore + Withdrawable;
    impl ProviderStructure {
        pub fn new<S: ProviderStructureReqs>(structure: S) -> Self {
            Self(EasyStructure::new(structure.into()), PhantomData)
        }
    }

    impl Withdrawable for ProviderStructure {
        fn withdrawable(&self) -> &dyn screeps::Withdrawable { self.0.structure_object().as_withdrawable().unwrap() }
    }
}