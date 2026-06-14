use std::{hash::Hash, marker::PhantomData};

use anyhow::anyhow;
use screeps::{Creep, HasId, HasPosition, HasStore, ObjectId, Position, Resource, ResourceType, Ruin, SharedCreepProperties, Store, Structure, StructureObject, Tombstone, Transferable, Withdrawable};
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsCast;

use crate::{safeid::{DO, GetSafeID, IDKind, SafeID, SafeIDs, TryFromUnsafe, TryMakeSafe, UnsafeIDs}};

pub trait TruckStopPos { fn pos(&self) -> Position; }

pub trait GetResourceAvaliable { fn get_resource_avaliable(&self, ty: ResourceType) -> u32; }
pub trait Withdraw { fn creep_withdraw(&self, creep: &Creep, ty: ResourceType) -> anyhow::Result<()>; }
pub trait Provide: GetResourceAvaliable + Withdraw + TruckStopPos {}
impl Provide for ProviderTruckStop {}
impl Provide for TruckStop<Provider, Structure> {}
impl Provide for TruckStop<Provider, Creep> {}
impl Provide for TruckStop<Provider, Ruin> {}
impl Provide for TruckStop<Provider, Resource> {}
impl Provide for TruckStop<Provider, Tombstone> {}

pub trait GetResourceFree { fn get_resource_free(&self, ty: ResourceType) -> u32; }
pub trait Transfer { fn creep_transfer(&self, creep: &Creep, ty: ResourceType) -> anyhow::Result<()>; }
pub trait Consume: GetResourceAvaliable + GetResourceFree + Transfer + TruckStopPos {}
impl Consume for ConsumerTruckStop {}
impl Consume for TruckStop<Consumer, Structure> {}
impl Consume for TruckStop<Consumer, Creep> {}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
#[serde(bound(deserialize = "TruckStop<Provider, Ruin, I> : DO, TruckStop<Provider, Resource, I> : DO, TruckStop<Provider, Tombstone, I> : DO, TruckStop<Provider, Structure, I> : DO, TruckStop<Provider, Creep, I> : DO"))]
pub enum ProviderTruckStop<I: IDKind = SafeIDs> {
    Ruin(TruckStop<Provider, Ruin, I>),
    Resource(TruckStop<Provider, Resource, I>),
    Tombstone(TruckStop<Provider, Tombstone, I>),
    Structure(TruckStop<Provider, Structure, I>),
    Creep(TruckStop<Provider, Creep, I>)
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
    pub fn get_provide(&self) -> &dyn Provide {
        match self {
            ProviderTruckStop::Ruin(truck_stop) => truck_stop,
            ProviderTruckStop::Resource(truck_stop) => truck_stop,
            ProviderTruckStop::Tombstone(truck_stop) => truck_stop,
            ProviderTruckStop::Structure(truck_stop) => truck_stop,
            ProviderTruckStop::Creep(truck_stop) => truck_stop
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
#[serde(bound(deserialize = "TruckStop<Consumer, Structure, I> : DO, TruckStop<Consumer, Creep, I> : DO"))]
pub enum ConsumerTruckStop<I: IDKind = SafeIDs> {
    Structure(TruckStop<Consumer, Structure, I>),
    Creep(TruckStop<Consumer, Creep, I>)
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
    pub fn get_consume(&self) -> &dyn Consume {
        match self {
            ConsumerTruckStop::Structure(truck_stop) => truck_stop,
            ConsumerTruckStop::Creep(truck_stop) => truck_stop
        }
    }
}

impl GetResourceAvaliable for ProviderTruckStop {
    fn get_resource_avaliable(&self, ty: ResourceType) -> u32 {
        self.get_provide().get_resource_avaliable(ty)
    }
}

impl Withdraw for ProviderTruckStop {
    fn creep_withdraw(&self, creep: &Creep, ty: ResourceType) -> anyhow::Result<()> {
        self.get_provide().creep_withdraw(creep, ty)
    }
}

impl TruckStopPos for ProviderTruckStop {
    fn pos(&self) -> Position {
        self.get_provide().pos()
    }
}

impl GetResourceAvaliable for ConsumerTruckStop {
    fn get_resource_avaliable(&self, ty: ResourceType) -> u32 {
        self.get_consume().get_resource_avaliable(ty)
    }
}

impl GetResourceFree for ConsumerTruckStop {
    fn get_resource_free(&self, ty: ResourceType) -> u32 {
        self.get_consume().get_resource_free(ty)
    }
}

impl Transfer for ConsumerTruckStop {
    fn creep_transfer(&self, creep: &Creep, ty: ResourceType) -> anyhow::Result<()> {
        self.get_consume().creep_transfer(creep, ty)
    }
}

impl TruckStopPos for ConsumerTruckStop {
    fn pos(&self) -> Position {
        self.get_consume().pos()
    }
}

pub trait TruckStopType {}

#[derive(Debug, Clone, PartialEq, Eq, Hash)] pub struct Consumer { }
impl TruckStopType for Consumer {}

#[derive(Debug, Clone, PartialEq, Eq, Hash)] pub struct Provider { }
impl TruckStopType for Provider {}

pub trait OtherEntity: JsCast + HasId + HasPosition {}
impl OtherEntity for Ruin {}
impl OtherEntity for Resource {}
impl OtherEntity for Tombstone {}

pub trait NormalOtherEntity: OtherEntity + HasStore + Withdrawable {}
impl NormalOtherEntity for Ruin {}
impl NormalOtherEntity for Tombstone {}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(bound(serialize = "", deserialize = "I::ID<E> : DO"))]
pub struct TruckStop<T, E:, I: IDKind = SafeIDs> {
    pub id: I::ID<E>,
    phantom: PhantomData<T>
}

impl<T, E> TryFromUnsafe for TruckStop<T, E> where ObjectId<E> : TryMakeSafe<SafeID<E>> {
    type Unsafe = TruckStop<T, E, UnsafeIDs>;

    fn try_from_unsafe(us: Self::Unsafe) -> Option<Self> {
        Some(Self { 
            id: us.id.try_make_safe()?, 
            phantom: PhantomData
        })
    }
}

impl<T, E, I: IDKind> Hash for TruckStop<T, E, I> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);

    }
}

impl<T, E, I: IDKind> Eq for TruckStop<T, E, I> { }
impl<T, E, I: IDKind> PartialEq for TruckStop<T, E, I> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl<T: TruckStopType, E: HasPosition> TruckStopPos for TruckStop<T, E> {
    fn pos(&self) -> Position {
        self.id.pos()
    }
}

impl<T : TruckStopType> TruckStop<T, Structure> {
    fn from_structure<S: Into<Structure>>(structure: S) -> Self {
        let structure = structure.into();
        Self { id: structure.safe_id(), phantom: PhantomData }
    }
}

pub trait ProviderStructureReqs = Withdrawable + HasStore + Into<Structure>;
impl TruckStop<Provider, Structure> {
    pub fn new<S: ProviderStructureReqs>(structure: S) -> Self {
        Self::from_structure(structure)
    }
}
pub trait ConsumerStructureReqs = Transferable + HasStore + Into<Structure>;
impl TruckStop<Consumer, Structure> {
    pub fn new<S: ConsumerStructureReqs>(structure: S) -> Self {
        Self::from_structure(structure)
    }
}

impl<E : OtherEntity + GetSafeID> TruckStop<Provider, E> {
    pub fn new(entity: &E) -> Self {
        Self { 
            id: entity.safe_id(), 
            phantom: PhantomData 
        }
    }
}

impl<T : TruckStopType> TruckStop<T, Creep> {
    pub fn new(creep: SafeID<Creep>) -> Self {
        Self { id: creep, phantom: PhantomData }
    }
}


trait ResolveStore { fn store(&self) -> Store; }
impl<T: TruckStopType> ResolveStore for TruckStop<T, Structure> {
    fn store(&self) -> Store {
        StructureObject::from(self.id.as_ref().clone()).as_has_store().map(HasStore::store).unwrap()
    }
}

impl<T: TruckStopType> ResolveStore for TruckStop<T, Creep> {
    fn store(&self) -> Store {
        self.id.store()
    }
}

impl<T: TruckStopType, E : NormalOtherEntity> ResolveStore for TruckStop<T, E> {
    fn store(&self) -> Store {
        self.id.store()
    }
}

impl<E> GetResourceAvaliable for TruckStop<Provider, E> where Self : ResolveStore {
    fn get_resource_avaliable(&self, ty: ResourceType) -> u32 {
        self.store().get_used_capacity(Some(ty))
    }
}

impl GetResourceAvaliable for TruckStop<Provider, Resource> {
    fn get_resource_avaliable(&self, ty: ResourceType) -> u32 {
        if self.id.resource_type() == ty {
            self.id.amount()
        } else {
            0
        }
    }
}

impl<E> GetResourceAvaliable for TruckStop<Consumer, E> where Self : ResolveStore {
    fn get_resource_avaliable(&self, ty: ResourceType) -> u32 {
        self.store().get_used_capacity(Some(ty))
    }
}

impl<E> GetResourceFree for TruckStop<Consumer, E> where Self : ResolveStore {
    fn get_resource_free(&self, ty: ResourceType) -> u32 {
        self.store().get_free_capacity(Some(ty)) as u32
    }
}

impl Transfer for TruckStop<Consumer, Structure> {
    fn creep_transfer(&self, creep: &Creep, ty: ResourceType) -> anyhow::Result<()> {
        let structure = StructureObject::from(self.id.as_ref().clone());
        Ok(creep.transfer(structure.as_transferable().ok_or(anyhow!("Entity is not transferable"))?, ty, None)?)
    }
}

impl Transfer for TruckStop<Consumer, Creep> {
    fn creep_transfer(&self, creep: &Creep, ty: ResourceType) -> anyhow::Result<()> {
        Ok(creep.transfer(self.id.as_ref(), ty, None)?)
    }
}

impl Withdraw for TruckStop<Provider, Structure> {
    fn creep_withdraw(&self, creep: &Creep, ty: ResourceType) -> anyhow::Result<()> {
        let structure = StructureObject::from(self.id.as_ref().clone());
        Ok(creep.withdraw(structure.as_withdrawable().ok_or(anyhow!("Entity is not withdrawable"))?, ty, None)?)
    }
}

impl Withdraw for TruckStop<Provider, Creep> {
    fn creep_withdraw(&self, creep: &Creep, ty: ResourceType) -> anyhow::Result<()> {
        Ok(self.id.transfer(creep, ty, None)?)
    }
}

impl<E : NormalOtherEntity> Withdraw for TruckStop<Provider, E> {
    fn creep_withdraw(&self, creep: &Creep, ty: ResourceType) -> anyhow::Result<()> {
        Ok(creep.withdraw(&*self.id, ty, None)?)
    }
}

impl Withdraw for TruckStop<Provider, Resource> {
    fn creep_withdraw(&self, creep: &Creep, ty: ResourceType) -> anyhow::Result<()> {
        if self.id.resource_type() != ty { return Err(anyhow!("Resource has wrong type. Expected {ty}, found {}", self.id.resource_type())) }
        Ok(creep.pickup(&self.id)?)
    }
}