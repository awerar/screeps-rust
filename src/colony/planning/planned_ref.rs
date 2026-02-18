use std::{cell::RefCell, marker::PhantomData};

use derive_deref::Deref;
use screeps::{ConstructionSite, MaybeHasId, ObjectId, OwnedStructureProperties, Position, RawObjectId, Room, RoomXY, StructureContainer, StructureExtension, StructureLink, StructureObject, StructureSpawn, StructureStorage, StructureTerminal, StructureType, look};
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsCast;

pub trait StructureRefReq = JsCast + MaybeHasId + ConstructionType where StructureObject : TryInto<Self>;
pub trait ResolvableRef<T> { fn resolve(&self) -> Option<T>; }
pub trait ResolvableRefs<T> { fn resolve(&self) -> Vec<T>; }

pub trait ResolvableStructureRef<T> { fn resolve_structure(&self) -> Option<T>; }
impl<R: ResolvableRef<T>, T: StructureRefReq> ResolvableStructureRef<T> for R {
    fn resolve_structure(&self) -> Option<T> {
        self.resolve()
    }
}

pub trait ResolvableSiteRef { fn resolve_site(&self) -> Option<ConstructionSite>; }
impl<R: ResolvableRef<ConstructionSite>> ResolvableSiteRef for R {
    fn resolve_site(&self) -> Option<ConstructionSite> {
        self.resolve()
    }
}

pub trait ResolvableStructureRefs<T> { fn resolve_structures(&self) -> Vec<T>; }
impl<R: ResolvableRefs<T>, T: StructureRefReq> ResolvableStructureRefs<T> for R {
    fn resolve_structures(&self) -> Vec<T> {
        self.resolve()
    }
}

pub trait ResolvableSiteRefs<T> { fn resolve_sites(&self) -> Vec<T>; }
impl<R: ResolvableRefs<T>, T: StructureRefReq> ResolvableSiteRefs<T> for R {
    fn resolve_sites(&self) -> Vec<T> {
        self.resolve()
    }
}

#[derive(Serialize, Deserialize, Default, Deref, Clone)]
#[serde(bound = "")]
pub struct PlannedStructureRefs<T>(pub Vec<PlannedStructureRef<T>>);

impl<T: StructureRefReq> PlannedStructureRefs<T> {
    pub fn are_completed(&self) -> bool {
        self.0.iter().all(PlannedStructureRef::is_complete)
    }
}

impl<T: StructureRefReq> ResolvableRefs<T> for PlannedStructureRefs<T> {
    fn resolve(&self) -> Vec<T> {
        self.0.iter().filter_map(PlannedStructureRef::resolve).collect()
    }
}

impl<T: StructureRefReq> ResolvableRefs<ConstructionSite> for PlannedStructureRefs<T> {
    fn resolve(&self) -> Vec<ConstructionSite> {
        self.0.iter().filter_map(PlannedStructureRef::resolve).collect()
    }
}

#[derive(Serialize, Deserialize, Clone, Default, Deref)]
#[serde(bound = "")]
pub struct OptionalPlannedStructureRef<T>(pub Option<PlannedStructureRef<T>>);

impl<T: StructureRefReq> OptionalPlannedStructureRef<T> {
    pub fn is_complete(&self) -> bool {
        self.0.as_ref().is_some_and(PlannedStructureRef::is_complete)
    }
}

impl<T: StructureRefReq> ResolvableRef<T> for OptionalPlannedStructureRef<T> {
    fn resolve(&self) -> Option<T> {
        self.0.as_ref().and_then(|structure| structure.resolve())
    }
}

impl<T: StructureRefReq> ResolvableRef<ConstructionSite> for OptionalPlannedStructureRef<T> {
    fn resolve(&self) -> Option<ConstructionSite> {
        self.0.as_ref().and_then(|structure| structure.resolve())
    }
}

impl<T> From<PlannedStructureRef<T>> for OptionalPlannedStructureRef<T> {
    fn from(value: PlannedStructureRef<T>) -> Self {
        Self(Some(value))
    }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(bound = "")]
pub struct PlannedStructureRef<T> {
    pub pos: Position,

    pub structure: PlannedStructureBuiltRef<T>,
    pub site: PlannedStructureSiteRef<T>
}

impl<T> PlannedStructureRef<T> {
    pub fn new(pos: RoomXY, room: &Room) -> Self {
        let pos = Position::new(pos.x, pos.y, room.name());

        Self {
            pos,
            structure: PlannedStructureBuiltRef::new(pos),
            site: PlannedStructureSiteRef::new(pos),
        }
    }
}

impl<T: StructureRefReq> PlannedStructureRef<T> {
    pub fn is_complete(&self) -> bool {
        self.structure.resolve().is_some()
    }

    pub fn is_being_built(&self) -> bool {
        self.site.resolve().is_some()
    }

    pub fn is_empty(&self) -> bool {
        !self.is_complete() && !self.is_being_built()
    }
}

impl<T : StructureRefReq> ResolvableRef<T> for PlannedStructureRef<T> {
    fn resolve(&self) -> Option<T> {
        self.structure.resolve()
    }
}

impl<T : StructureRefReq> ResolvableRef<ConstructionSite> for PlannedStructureRef<T> {
    fn resolve(&self) -> Option<ConstructionSite> {
        self.site.resolve()
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct PlannedStructureBuiltRef<T> {
    pub pos: Position,

    id: RefCell<Option<RawObjectId>>,
    phantom: PhantomData<fn() -> T>
}

impl<T> PlannedStructureBuiltRef<T> {
    pub fn new(pos: Position) -> Self {
        Self { pos, id: RefCell::new(None), phantom: PhantomData }
    }
}

impl<T: StructureRefReq> ResolvableRef<T> for PlannedStructureBuiltRef<T> {
    fn resolve(&self) -> Option<T> {
        let id = *self.id.borrow();
        if let Some(id) = id {
            if let Some(structure) = ObjectId::<T>::from(id).resolve() {
                return Some(structure);
            }

            self.id.replace(None);
        }

        let structure = self.pos.look_for(look::STRUCTURES).ok()?.into_iter()
            .filter(|structure| structure.as_owned().is_none_or(OwnedStructureProperties::my))
            .flat_map(TryInto::try_into)
            .next()?;

        if let Some(raw_id) = structure.try_raw_id() {
            self.id.replace(Some(raw_id));
        }

        Some(structure)
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct PlannedStructureSiteRef<T> {
    pub pos: Position,

    id: RefCell<Option<ObjectId<ConstructionSite>>>,
    phantom: PhantomData<T>
}

pub trait ConstructionType { fn structure_type() -> StructureType; }
impl ConstructionType for StructureContainer { fn structure_type() -> StructureType { StructureType::Container } }
impl ConstructionType for StructureSpawn { fn structure_type() -> StructureType { StructureType::Spawn } }
impl ConstructionType for StructureStorage { fn structure_type() -> StructureType { StructureType::Storage } }
impl ConstructionType for StructureExtension { fn structure_type() -> StructureType { StructureType::Extension } }
impl ConstructionType for StructureLink { fn structure_type() -> StructureType { StructureType::Link } }
impl ConstructionType for StructureTerminal { fn structure_type() -> StructureType { StructureType::Terminal } }

impl<T> PlannedStructureSiteRef<T> {
    pub fn new(pos: Position) -> Self {
        Self { pos, id: RefCell::new(None), phantom: PhantomData }
    }
}

impl<T: ConstructionType> ResolvableRef<ConstructionSite> for PlannedStructureSiteRef<T> {
    fn resolve(&self) -> Option<ConstructionSite> {
        let id = *self.id.borrow();
        if let Some(id) = id {
            if let Some(site) = id.resolve() {
                return Some(site);
            }

            self.id.replace(None);
        }

        let site = self.pos.look_for(look::CONSTRUCTION_SITES).ok()?.into_iter()
            .find(|site| site.my() && site.structure_type() == T::structure_type())?;

        if let Some(id) = site.try_id() {
            self.id.replace(Some(id));
        }

        Some(site)
    }
}