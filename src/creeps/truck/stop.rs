use std::hash::Hash;

use anyhow::anyhow;
use screeps::{Creep, HasPosition, Position, Resource, ResourceType, Ruin, Tombstone};
use serde::{Deserialize, Serialize};

use crate::{creeps::{truck::stop::safe_structure::{ConsumerStructure, ProviderStructure}, virtual_creep::{IntentError, VirtualCreep}}, domain_traits::{HasStore, Transferable}, safeid::{DO, IDKind, SafeIDs, TryFromUnsafe, TryMakeSafe, UnsafeIDs}};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
#[serde(bound(deserialize = "I::ID<Ruin> : DO, I::ID<Resource> : DO, I::ID<Tombstone> : DO, ProviderStructure<I> : DO, I::ID<Creep> : DO"))]
pub enum ProviderTruckStop<I: IDKind = SafeIDs> {
    Ruin(I::ID<Ruin>),
    Resource(I::ID<Resource>),
    Tombstone(I::ID<Tombstone>),
    Structure(ProviderStructure<I>),
    Creep(I::ID<Creep>)
}

impl TryFromUnsafe for ProviderTruckStop {
    type Unsafe = ProviderTruckStop<UnsafeIDs>;

    fn try_from_unsafe(us: Self::Unsafe) -> Option<Self> {
        Some(match us {
            Self::Unsafe::Ruin(x) => Self::Ruin(x.try_make_safe()?),
            Self::Unsafe::Resource(x) => Self::Resource(x.try_make_safe()?),
            Self::Unsafe::Tombstone(x) => Self::Tombstone(x.try_make_safe()?),
            Self::Unsafe::Structure(x) => Self::Structure(x.try_make_safe()?),
            Self::Unsafe::Creep(x) => Self::Creep(x.try_make_safe()?),
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
            Self::Ruin(id) => Ok(creep.withdraw(id.as_ref(), ty, None)?),
            Self::Tombstone(id) => Ok(creep.withdraw(id.as_ref(), ty, None)?),
            Self::Creep(id) => Ok(creep.transfer_from(id, ty, None)?),
            Self::Structure(id) => creep.withdraw(id, ty, None),
            Self::Resource(id) => 
                if id.resource_type() == ty { 
                    Ok(creep.pickup(id)?) 
                } else { 
                    Err(anyhow!("Resource pile does not contain {ty}").into()) 
                },
        }
    }

}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
#[serde(bound(deserialize = "ConsumerStructure<I> : DO, I::ID<Creep> : DO"))]
pub enum ConsumerTruckStop<I: IDKind = SafeIDs> {
    Structure(ConsumerStructure<I>),
    Creep(I::ID<Creep>)
}

impl TryFromUnsafe for ConsumerTruckStop {
    type Unsafe = ConsumerTruckStop<UnsafeIDs>;

    fn try_from_unsafe(us: Self::Unsafe) -> Option<Self> {
        Some(match us {
            Self::Unsafe::Structure(x) => Self::Structure(x.try_make_safe()?),
            Self::Unsafe::Creep(x) => Self::Creep(x.try_make_safe()?),
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

    use screeps::{HasPosition, Position, Store, Structure};
    use serde::{Deserialize, Serialize};

    use crate::{domain_traits::{HasStore, Transferable, Withdrawable}, safeid::{DO, GetSafeID, IDKind, SafeIDs, TryFromUnsafe, TryMakeSafe, UnsafeIDs}, utils::EasyStructure};

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
    #[serde(bound(deserialize = "EasyStructure<I> : DO"))]
    pub struct SafeStructure<T, I: IDKind = SafeIDs>(EasyStructure<I>, PhantomData<T>);

    impl<T> TryFromUnsafe for SafeStructure<T> {
        type Unsafe = SafeStructure<T, UnsafeIDs>;
    
        fn try_from_unsafe(us: Self::Unsafe) -> Option<Self> {
            Some(SafeStructure(us.0.try_make_safe()?, PhantomData))
        }
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)] pub struct Consumer;
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)] pub struct Provider;

    pub type ConsumerStructure<I = SafeIDs> = SafeStructure<Consumer, I>;
    pub type ProviderStructure<I = SafeIDs> = SafeStructure<Provider, I>;

    impl<T> SafeStructure<T> {
        pub fn pos(&self) -> Position { self.0.pos() }
    }

    impl<T> HasStore for SafeStructure<T> {
        fn store(&self) -> Store { self.0.structure_object().as_has_store().unwrap().store() }
    }

    pub trait ConsumerStructureReqs = Into<Structure> + HasStore + Transferable;
    impl ConsumerStructure {
        pub fn new<S: ConsumerStructureReqs>(structure: S) -> Self {
            Self(EasyStructure::new(structure.into().safe_id()), PhantomData)
        }
    }

    impl Transferable for ConsumerStructure {
        fn transferable(&self) -> &dyn screeps::Transferable { self.0.structure_object().as_transferable().unwrap() }
    }

    pub trait ProviderStructureReqs = Into<Structure> + HasStore + Withdrawable;
    impl ProviderStructure {
        pub fn new<S: ProviderStructureReqs>(structure: S) -> Self {
            Self(EasyStructure::new(structure.into().safe_id()), PhantomData)
        }
    }

    impl Withdrawable for ProviderStructure {
        fn withdrawable(&self) -> &dyn screeps::Withdrawable { self.0.structure_object().as_withdrawable().unwrap() }
    }
}