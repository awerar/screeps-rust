use std::{fmt::Debug, hash::Hash};

use screeps::ResourceType;
use serde::{Serialize, de::DeserializeOwned};

pub trait HasStore {
    fn store(&self) -> screeps::Store;
}

impl<T: screeps::HasStore> HasStore for T {
    fn store(&self) -> screeps::Store {
        screeps::HasStore::store(self)
    }
}

pub trait HasStoreExt {
    fn capacity(&self, ty: Option<ResourceType>) -> u32;
    fn used_capacity(&self, ty: Option<ResourceType>) -> u32;
    fn free_capacity(&self, ty: Option<ResourceType>) -> u32;
}

impl<T: HasStore> HasStoreExt for T {
    fn capacity(&self, ty: Option<ResourceType>) -> u32 { self.store().get_capacity(ty) }
    fn used_capacity(&self, ty: Option<ResourceType>) -> u32 { self.store().get_used_capacity(ty) }
    fn free_capacity(&self, ty: Option<ResourceType>) -> u32 { self.store().get_free_capacity(ty).try_into().unwrap_or(0) }
}

pub trait EnergyStoreAccessors {
    fn energy_capacity(&self) -> u32;
    fn used_energy_capacity(&self) -> u32;
    fn free_energy_capacity(&self) -> u32;
}

impl<T: HasStoreExt> EnergyStoreAccessors for T {
    fn energy_capacity(&self) -> u32 { self.capacity(Some(ResourceType::Energy)) }
    fn used_energy_capacity(&self) -> u32 { self.used_capacity(Some(ResourceType::Energy)) }
    fn free_energy_capacity(&self) -> u32 { self.free_capacity(Some(ResourceType::Energy)) }
}

pub trait Transferable: HasStoreExt {
    fn transferable(&self) -> &dyn screeps::Transferable;
}

impl<T: screeps::Transferable + screeps::HasStore> Transferable for T {
    fn transferable(&self) -> &dyn screeps::Transferable { self }
}

pub trait Withdrawable: HasStoreExt {
    fn withdrawable(&self) -> &dyn screeps::Withdrawable;
}

impl<T: screeps::Withdrawable + screeps::HasStore> Withdrawable for T {
    fn withdrawable(&self) -> &dyn screeps::Withdrawable { self }
}

pub trait IdReqs = DeserializeOwned + Serialize + Hash + Eq + Ord + Clone + Copy + Debug;

pub trait HasId: Sized {
    type Id: IdReqs;

    fn id(&self) -> Self::Id;
}

pub trait MaybeHasId: Sized {
    type Id: IdReqs;

    fn try_id(&self) -> Option<Self::Id>;
}

pub mod screeps_objects {
    #[allow(clippy::wildcard_imports)]
    use screeps::{objects::*, ObjectId};
    use thiserror::Error;
    use super::{HasId, MaybeHasId};

    #[derive(Error, Debug)]
    #[error("Unable to resolve {0}")]
    pub struct IdResolutionError<T>(pub ObjectId<T>);

    macro_rules! has_id_entities {
        ($($ty:ty),* $(,)?) => {
            $(
                impl HasId for $ty {
                    type Id = ObjectId<Self>;
                    fn id(&self) -> Self::Id { screeps::HasId::id(&self) }
                }

                impl $crate::check::CheckFrom for $ty {
                    type Unchecked = ObjectId<$ty>;
                    type Err = IdResolutionError<$ty>;

                    fn check_from(us: Self::Unchecked) -> Result<Self, Self::Err> {
                        us.resolve().ok_or_else(|| IdResolutionError(us))
                    }
                }
            )*
        };
    }

    macro_rules! maybe_has_id_entities {
        ($($ty:ty),* $(,)?) => {
            $(
                impl MaybeHasId for $ty {
                    type Id = ObjectId<Self>;
                    fn try_id(&self) -> Option<Self::Id> { screeps::MaybeHasId::try_id(&self) }
                }
            )*
        };
    }

    has_id_entities!(
        Deposit, Mineral, Nuke, PowerCreep, Resource, Ruin, Source, Structure,
        StructureContainer, StructureController, StructureExtension, 
        StructureExtractor, StructureFactory, StructureLab, StructureLink, 
        StructureObserver, StructurePowerBank, StructurePowerSpawn, 
        StructurePortal, StructureRampart, StructureRoad, StructureSpawn, 
        StructureStorage, StructureTerminal, StructureTower, 
        StructureWall, Tombstone
    );

    maybe_has_id_entities!(Creep, ConstructionSite);
}

impl<T: HasId> MaybeHasId for T {
    type Id = T::Id;
    fn try_id(&self) -> Option<Self::Id> { Some(self.id()) }
}