use std::{fmt::Debug, hash::Hash, marker::PhantomData};

use derive_where::derive_where;
use screeps::{Creep, ResourceType, game};
use serde::{Serialize, de::DeserializeOwned};
use thiserror::Error;
use wasm_bindgen::JsCast;

use crate::{check::{Check, CheckFrom}, ids::{CheckState, Checked, Unchecked}};

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

pub trait Repairable {
    fn repairable(&self) -> &dyn screeps::Repairable;
}

impl<T: screeps::Repairable> Repairable for T {
    fn repairable(&self) -> &dyn screeps::Repairable { self }
}

pub trait HasName {
    fn name(&self) -> String;
}

impl HasName for Creep {
    fn name(&self) -> String {
        screeps::SharedCreepProperties::name(self)
    }
}

pub trait ResolvableId {
    type Target;

    fn resolve(&self) -> Self::Target;
}

pub trait HasId: Sized {
    type Id: Serialize + Hash + Eq + Ord + Clone + Debug + CheckFrom;

    fn id(&self) -> Self::Id;
}

#[derive_where(Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct ObjectId<T, S: CheckState = Checked> {
    id: screeps::ObjectId<T>,
    phantom: PhantomData<S>
}

impl<T: screeps::MaybeHasId + JsCast> ResolvableId for ObjectId<T> {
    type Target = T;

    fn resolve(&self) -> Self::Target {
        self.id.resolve().unwrap()
    }
}

impl<T: screeps::HasId> ObjectId<T> {
    pub fn new(x: &T) -> Self {
        ObjectId {
            id: x.id(),
            phantom: PhantomData
        }
    }
}

impl<T: screeps::MaybeHasId> ObjectId<T> {
    pub fn try_new(x: &T) -> Option<Self> {
        Some(ObjectId {
            id: x.try_id()?,
            phantom: PhantomData
        })
    }
}

#[derive(Error, Debug)]
#[error("Unable to resolve {0}")]
pub struct IdResolutionError<T>(pub screeps::ObjectId<T>);

impl<T: JsCast + screeps::MaybeHasId> CheckFrom for ObjectId<T> {
    type Unchecked = ObjectId<T, Unchecked>;
    type Err = IdResolutionError<T>;

    fn check_from(uc: Self::Unchecked) -> Result<Self, Self::Err> {
        if uc.id.resolve().is_none() { return Err(IdResolutionError(uc.id)) }

        Ok(Self {
            id: uc.id,
            phantom: PhantomData
        })
    }
}

pub mod screeps_objects {
    #[allow(clippy::wildcard_imports)]
    use screeps::objects::*;
    use super::{HasId, ObjectId};

    macro_rules! has_id_entities {
        ($($ty:ty),* $(,)?) => {
            $(
                impl HasId for $ty {
                    type Id = ObjectId<Self>;
                    fn id(&self) -> Self::Id { ObjectId::new(self) }
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
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[derive_where(Serialize, Deserialize, Clone; ObjectId<Creep, S>)]
pub enum CreepId<S: CheckState = Checked> {
    Id(ObjectId<Creep, S>),
    Name(String)
}

impl ResolvableId for CreepId {
    type Target = Creep;

    fn resolve(&self) -> Self::Target {
        match self {
            CreepId::Id(id) => id.resolve(),
            CreepId::Name(name) => game::creeps().get(name.clone()).unwrap(),
        }
    }
}

impl HasId for Creep {
    type Id = CreepId;

    fn id(&self) -> Self::Id {
        ObjectId::try_new(self)
            .map_or_else(|| CreepId::Name(self.name()), CreepId::Id)
    }
}

#[expect(unused)]
pub enum CreepIdCheckError {
    Id(IdResolutionError<Creep>),
    UnknownName(String)
}

impl CheckFrom for CreepId {
    type Unchecked = CreepId<Unchecked>;
    type Err = CreepIdCheckError;

    fn check_from(uc: Self::Unchecked) -> Result<Self, Self::Err> {
        Ok(match uc {
            CreepId::Id(id) => 
                Self::Id(id.check().map_err(CreepIdCheckError::Id)?),
            CreepId::Name(name) => {
                let Some(creep) = game::creeps().get(name.clone()) else {
                    return Err(CreepIdCheckError::UnknownName(name))
                };

                ObjectId::try_new(&creep).map_or(CreepId::Name(name), CreepId::Id)
            },
        })
    }
}