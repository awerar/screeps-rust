use std::hash::Hash;

use screeps::{Creep, HasPosition, Position, Resource, ResourceType, Ruin, SharedCreepProperties, Store, Tombstone};
use serde::{Deserialize, Serialize};

use crate::{creeps::truck::stop::safe_structure::{ConsumerStructure, ProviderStructure}, safeid::{DO, IDKind, SafeIDs, TryFromUnsafe, TryMakeSafe, UnsafeIDs}};

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
            Self::Resource(id) => id.amount(),
        }
    }

    pub fn creep_withdraw(&self, creep: &Creep, ty: ResourceType) -> anyhow::Result<()> { 
        match self {
            Self::Ruin(id) => Ok(creep.withdraw(id.as_ref(), ty, None)?),
            Self::Tombstone(id) => Ok(creep.withdraw(id.as_ref(), ty, None)?),
            Self::Creep(id) => Ok(id.transfer(creep, ty, None)?),
            Self::Structure(id) => id.creep_withdraw(creep, ty),
            Self::Resource(id) => Ok(creep.pickup(id)?),
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

    pub fn store(&self) -> Store {
        match self {
            Self::Structure(id) => id.store(),
            Self::Creep(id) => id.store(),
        }
    }

    pub fn get_resource_avaliable(&self, ty: ResourceType) -> u32 { 
        self.store().get_used_capacity(Some(ty))
    }

    pub fn get_resource_free(&self, ty: ResourceType) -> u32 { 
        self.store().get_free_capacity(Some(ty)) as u32
    }

    pub fn creep_transfer(&self, creep: &Creep, ty: ResourceType) -> anyhow::Result<()> {
        match self {
            Self::Structure(id) => id.creep_transfer(creep, ty),
            Self::Creep(id) => Ok(creep.transfer(&**id, ty, None)?),
        }
    }

}

pub mod safe_structure {
#   ![allow(clippy::missing_panics_doc)]

    use std::marker::PhantomData;

    use anyhow::Ok;
    use screeps::{Creep, HasPosition, HasStore, Position, ResourceType, SharedCreepProperties, Store, Structure, StructureObject, Transferable, Withdrawable};
    use serde::{Deserialize, Serialize};

    use crate::safeid::{GetSafeID, IDKind, SafeIDs, TryFromUnsafe, TryMakeSafe, UnsafeIDs};

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
    pub struct SafeStructure<T, I: IDKind = SafeIDs>(I::ID<Structure>, PhantomData<T>);

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
        fn structure_object(&self) -> StructureObject {
            StructureObject::from(self.0.as_ref().clone())
        }

        pub fn pos(&self) -> Position { self.0.pos() }

        pub fn store(&self) -> Store { 
            self.structure_object().as_has_store().map(HasStore::store).unwrap()
        }
    }

    pub trait ConsumerStructureReqs = Into<Structure> + HasStore + Transferable;
    impl ConsumerStructure {
        pub fn new<S: ConsumerStructureReqs>(structure: S) -> Self {
            Self(structure.into().safe_id(), PhantomData)
        }

        pub fn creep_transfer(&self, creep: &Creep, ty: ResourceType) -> anyhow::Result<()> {
            Ok(creep.transfer(self.structure_object().as_transferable().unwrap(), ty, None)?)
        }
    }

    pub trait ProviderStructureReqs = Into<Structure> + HasStore + Withdrawable;
    impl ProviderStructure {
        pub fn new<S: ProviderStructureReqs>(structure: S) -> Self {
            Self(structure.into().safe_id(), PhantomData)
        }

        pub fn creep_withdraw(&self, creep: &Creep, ty: ResourceType) -> anyhow::Result<()> {
            Ok(creep.withdraw(self.structure_object().as_withdrawable().unwrap(), ty, None)?)
        }
    }
}