use std::{fmt::Debug, hash::Hash, marker::PhantomData};

use derive_where::derive_where;
use screeps::{Creep, ResourceType, action_error_codes::{CreepRepairErrorCode, TransferErrorCode, WithdrawErrorCode}, game};
use serde::Serialize;
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
    fn transfer_from(&self, creep: &Creep, ty: ResourceType, amount: Option<u32>) -> Result<(), TransferErrorCode>;
}

impl<T: screeps::Transferable + screeps::HasStore> Transferable for T {
    fn transfer_from(&self, creep: &Creep, ty: ResourceType, amount: Option<u32>) -> Result<(), TransferErrorCode> {
        screeps::SharedCreepProperties::transfer(creep, self, ty, amount)
    }
}

pub trait Withdrawable: HasStoreExt {
    fn withdraw_to(&self, creep: &Creep, ty: ResourceType, amount: Option<u32>) -> Result<(), WithdrawErrorCode>;
}

impl<T: screeps::Withdrawable + screeps::HasStore> Withdrawable for T {
    fn withdraw_to(&self, creep: &Creep, ty: ResourceType, amount: Option<u32>) -> Result<(), WithdrawErrorCode> {
        screeps::SharedCreepProperties::withdraw(creep, self, ty, amount)
    }
}

pub trait Repairable: HasHits {
    fn repair_by(&self, creep: &Creep) -> Result<(), CreepRepairErrorCode>;
}

impl<T: screeps::Repairable> Repairable for T {
    fn repair_by(&self, creep: &Creep) -> Result<(), CreepRepairErrorCode> {
        creep.repair(self)
    }
}

pub trait HasHits {
    fn hits(&self) -> u32;
    fn hits_max(&self) -> u32;
}

impl<T: screeps::HasHits> HasHits for T {    
    fn hits(&self) -> u32 {
        screeps::HasHits::hits(self)
    }
    
    fn hits_max(&self) -> u32 {
        screeps::HasHits::hits_max(self)
    }
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
    type Id<S: CheckState>: Serialize + Hash + Eq + Ord + Clone + Debug;

    fn id(&self) -> Self::Id<Checked>;
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

impl<T> ObjectId<T> {
    pub fn screeps_id(&self) -> screeps::ObjectId<T> {
        self.id
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
                    type Id<S: crate::ids::CheckState> = ObjectId<Self, S>;
                    fn id(&self) -> Self::Id<crate::ids::Checked> { ObjectId::new(self) }
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

#[derive_where(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash; ObjectId<Creep, S>)]
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
    type Id<S: CheckState> = CreepId<S>;

    fn id(&self) -> Self::Id<Checked> {
        ObjectId::try_new(self)
            .map_or_else(|| CreepId::Name(self.name()), CreepId::Id)
    }
}

#[derive(Error, Debug)]
pub enum CreepIdCheckError {
    #[error(transparent)] Id(IdResolutionError<Creep>),
    #[error("Unknown name {0}")] UnknownName(String)
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