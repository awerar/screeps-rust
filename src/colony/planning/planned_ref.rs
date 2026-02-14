use std::{cell::RefCell, marker::PhantomData};

use screeps::{ConstructionSite, MaybeHasId, ObjectId, Position, RawObjectId, Room, RoomXY, StructureContainer, StructureExtension, StructureLink, StructureObject, StructureSpawn, StructureStorage, StructureTerminal, StructureType, look};
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsCast;

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
            pos: pos,
            structure: PlannedStructureBuiltRef::new(pos),
            site: PlannedStructureSiteRef::new(pos),
        }
    }
}

impl<T> PlannedStructureRef<T> where T : JsCast + MaybeHasId + ConstructionType, StructureObject : TryInto<T> {
    pub fn is_complete(&self) -> bool {
        self.structure.resolve().is_some()
    }

    pub fn is_being_built(&self) -> bool {
        self.site.resolve().is_some()
    }

    pub fn is_empty(&self) -> bool {
        !self.is_complete() && !self.is_being_built()
    }

    pub fn resolve(&self) -> Option<T> {
        self.structure.resolve()
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct PlannedStructureBuiltRef<T> {
    pub pos: Position,

    id: RefCell<Option<RawObjectId>>,
    phantom: PhantomData<fn() -> T>
}

impl<T> PlannedStructureBuiltRef<T> {
    fn new(pos: Position) -> Self {
        Self { pos, id: RefCell::new(None), phantom: PhantomData }
    }
}

impl<T> PlannedStructureBuiltRef<T> where T : JsCast + MaybeHasId + ConstructionType, StructureObject : TryInto<T> {
    pub fn resolve(&self) -> Option<T> {
        if let Some(id) = self.id.borrow().clone() {
            if let Some(structure) = ObjectId::<T>::from(id).resolve() {
                return Some(structure);
            } else {
                self.id.replace(None);
            }
        }

        let structure = self.pos.look_for(look::STRUCTURES).ok()?.into_iter()
            .filter(|structure| structure.as_owned().map_or(true, |x| x.my()))
            .flat_map(|structure| structure.try_into())
            .next();

        let Some(structure) = structure else { return None; };

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

pub trait ConstructionType {
    fn structure_type() -> StructureType;
}

impl ConstructionType for StructureContainer { fn structure_type() -> StructureType { StructureType::Container } }
impl ConstructionType for StructureSpawn { fn structure_type() -> StructureType { StructureType::Spawn } }
impl ConstructionType for StructureStorage { fn structure_type() -> StructureType { StructureType::Storage } }
impl ConstructionType for StructureExtension { fn structure_type() -> StructureType { StructureType::Extension } }
impl ConstructionType for StructureLink { fn structure_type() -> StructureType { StructureType::Link } }
impl ConstructionType for StructureTerminal { fn structure_type() -> StructureType { StructureType::Terminal } }

impl<T> PlannedStructureSiteRef<T> {
    fn new(pos: Position) -> Self {
        Self { pos, id: RefCell::new(None), phantom: PhantomData }
    }
}

impl<T> PlannedStructureSiteRef<T> where T : ConstructionType {
    pub fn resolve(&self) -> Option<ConstructionSite> {
        if let Some(id) = self.id.borrow().clone() {
            if let Some(site) = id.resolve() {
                return Some(site);
            } else {
                self.id.replace(None);
            }
        }

        let site = self.pos.look_for(look::CONSTRUCTION_SITES).ok()?.into_iter()
            .filter(|site| site.my() && site.structure_type() == T::structure_type())
            .next();

        let Some(site) = site else { return None; };

        if let Some(id) = site.try_id() {
            self.id.replace(Some(id));
        }

        Some(site)
    }
}