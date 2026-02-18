use std::{cell::RefCell, marker::PhantomData};

use derive_deref::Deref;
use screeps::{ConstructionSite, MaybeHasId, ObjectId, OwnedStructureProperties, Position, RawObjectId, Room, RoomXY, StructureContainer, StructureExtension, StructureLink, StructureObject, StructureSpawn, StructureStorage, StructureTerminal, StructureTower, StructureType, look};
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsCast;

pub trait StructureRefReq = JsCast + MaybeHasId + ConstructionType where StructureObject : TryInto<Self>;
pub trait ResolvableStructureRef { 
    type Structure;

    fn resolve(&self) -> Option<Self::Structure>; 
}
pub trait ResolvableSiteRef { fn resolve_site(&self) -> Option<ConstructionSite>; }

#[derive(Serialize, Deserialize, Default, Deref, Clone)]
#[serde(bound = "")]
pub struct PlannedStructureRefs<T>(pub Vec<PlannedStructureRef<T>>);

impl<T: StructureRefReq> PlannedStructureRefs<T> {
    #[expect(unused)]
    pub fn all_completed(&self) -> bool {
        self.0.iter().all(PlannedStructureRef::is_complete)
    }

    pub fn resolve(&self) -> Vec<T> {
        self.0.iter().filter_map(PlannedStructureRef::resolve).collect()
    }

    #[expect(unused)]
    pub fn resolve_sites(&self) -> Vec<ConstructionSite> {
        self.0.iter().filter_map(PlannedStructureRef::resolve_site).collect()
    }
}

#[derive(Serialize, Deserialize, Clone, Default, Deref)]
#[serde(bound = "")]
pub struct OptionalPlannedStructureRef<T>(pub Option<PlannedStructureRef<T>>);

impl<T: StructureRefReq> OptionalPlannedStructureRef<T> {
    #[expect(unused)]
    pub fn is_complete(&self) -> bool {
        self.0.as_ref().is_some_and(PlannedStructureRef::is_complete)
    }
}

impl<T: StructureRefReq> ResolvableStructureRef for OptionalPlannedStructureRef<T> {
    type Structure = T;

    fn resolve(&self) -> Option<T> {
        self.0.as_ref().and_then(ResolvableStructureRef::resolve)
    }
}

impl<T: StructureRefReq> ResolvableSiteRef for OptionalPlannedStructureRef<T> {
    fn resolve_site(&self) -> Option<ConstructionSite> {
        self.0.as_ref().and_then(ResolvableSiteRef::resolve_site)
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
        self.site.resolve_site().is_some()
    }

    #[expect(unused)]
    pub fn is_empty(&self) -> bool {
        !self.is_complete() && !self.is_being_built()
    }
}

impl<T : StructureRefReq> ResolvableStructureRef for PlannedStructureRef<T> {
    type Structure = T;

    fn resolve(&self) -> Option<T> {
        self.structure.resolve()
    }
}

impl<T : StructureRefReq> ResolvableSiteRef for PlannedStructureRef<T> {
    fn resolve_site(&self) -> Option<ConstructionSite> {
        self.site.resolve_site()
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

impl<T: StructureRefReq> ResolvableStructureRef for PlannedStructureBuiltRef<T> {
    type Structure = T;

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
impl ConstructionType for StructureTower { fn structure_type() -> StructureType { StructureType::Tower } }

impl<T> PlannedStructureSiteRef<T> {
    pub fn new(pos: Position) -> Self {
        Self { pos, id: RefCell::new(None), phantom: PhantomData }
    }
}

impl<T: ConstructionType> ResolvableSiteRef for PlannedStructureSiteRef<T> {
    fn resolve_site(&self) -> Option<ConstructionSite> {
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