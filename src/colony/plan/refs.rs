use std::cell::RefCell;

use derive_deref::Deref;
use derive_where::derive_where;
use option_entry::OptionEntry;
use screeps::{ConstructionSite, OwnedStructureProperties, Position, StructureContainer, StructureExtension, StructureExtractor, StructureFactory, StructureLab, StructureLink, StructureNuker, StructureObject, StructureObserver, StructurePowerSpawn, StructureRampart, StructureRoad, StructureSpawn, StructureStorage, StructureTerminal, StructureTower, StructureType, StructureWall, look};
use serde::{Deserialize, de::DeserializeOwned};

use crate::{check::FilterCheck, domain_traits::{ConstructionSiteId, HasCheckableId, HasId, HasResolvableId, ResolvableId}, ids::{CheckState, Checked, Unchecked}};

#[derive_where(Serialize, Deserialize, Default, Clone; PlannedStructureRef<T>)]
#[derive(Deref)]
pub struct PlannedStructureRefs<T: HasId>(pub Vec<PlannedStructureRef<T>>);

impl<T: HasResolvableId + HasStructureType + From<StructureObject>> PlannedStructureRefs<T> {
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

#[derive_where(Serialize, Deserialize, Clone, Default; PlannedStructureRef<T>)]
#[derive(Deref)]
pub struct OptionalPlannedStructureRef<T: HasId>(pub Option<PlannedStructureRef<T>>);

impl<T: HasResolvableId + HasStructureType + From<StructureObject>> OptionalPlannedStructureRef<T> {
    fn resolve(&self) -> Option<T> {
        self.0.as_ref().and_then(PlannedStructureRef::resolve)
    }

    pub fn is_complete(&self) -> bool {
        self.0.as_ref().is_some_and(PlannedStructureRef::is_complete)
    }

    fn resolve_site(&self) -> Option<ConstructionSite> {
        self.0.as_ref().and_then(PlannedStructureRef::resolve_site)
    }
}

impl<T: HasId> From<PlannedStructureRef<T>> for OptionalPlannedStructureRef<T> {
    fn from(value: PlannedStructureRef<T>) -> Self {
        Self(Some(value))
    }
}

#[derive_where(Serialize, Deserialize, Clone; T::Id<S>, S)]
pub struct PlannedStructureRef<T: HasId, S: CheckState = Checked> {
    pub pos: Position,

    structure: RefCell<Option<T::Id<S>>>,
    site: RefCell<Option<ConstructionSiteId<S>>>
}

impl<'de, T: HasCheckableId> Deserialize<'de> for PlannedStructureRef<T> 
where 
    T::Id<Unchecked> : DeserializeOwned 
{
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let uc = PlannedStructureRef::<T, Unchecked>::deserialize(deserializer)?;
        
        Ok(Self {
            pos: uc.pos,
            structure: RefCell::new(uc.structure.into_inner().filter_check().0),
            site: RefCell::new(uc.site.into_inner().filter_check().0),
        })
    }
}

impl<T: HasId> PlannedStructureRef<T> {
    pub fn new(pos: Position) -> Self {
        Self {
            pos,
            structure: RefCell::new(None),
            site: RefCell::new(None),
        }
    }
}

impl<T : HasResolvableId + HasStructureType + From<StructureObject>> PlannedStructureRef<T> {
    pub fn resolve(&self) -> Option<T> {
        match self.structure.borrow_mut().entry() {
            option_entry::Entry::Vacant(entry) => {
                let structure: T = find_structure(self.pos, T::STRUCTURE_TYPE)?.try_into().ok().unwrap();
                entry.insert_entry(structure.id());

                Some(structure)
            },
            option_entry::Entry::Occupied(entry) => Some(entry.get().resolve()),
        }
    }

    pub fn is_complete(&self) -> bool {
        self.resolve().is_some()
    }

    pub fn resolve_site(&self) -> Option<ConstructionSite> {
        match self.site.borrow_mut().entry() {
            option_entry::Entry::Vacant(entry) => {
                let site = find_site(self.pos, T::STRUCTURE_TYPE)?;
                entry.insert_entry(site.id());

                Some(site)
            },
            option_entry::Entry::Occupied(entry) => Some(entry.get().resolve()),
        }
    }

    pub fn is_being_built(&self) -> bool {
        self.resolve_site().is_some()
    }
}

fn find_site(pos: Position, ty: StructureType) -> Option<ConstructionSite> {
    pos.look_for(look::CONSTRUCTION_SITES).ok()?.into_iter()
        .find(|site| site.my() && site.structure_type() == ty)
}

fn find_structure(pos: Position, ty: StructureType) -> Option<StructureObject> {
    pos.look_for(look::STRUCTURES).ok()?.into_iter()
        .find(|structure| structure.as_owned().is_none_or(OwnedStructureProperties::my) && structure.structure_type() == ty)
}

pub trait HasStructureType { const STRUCTURE_TYPE: StructureType; }
macro_rules! structure_types {
    ($(($structure:path, $ty:ident)),* $(,)?) => {
        $(
            impl HasStructureType for $structure {
                const STRUCTURE_TYPE: StructureType = StructureType::$ty;
            }
        )*
    };
}

structure_types!(
    (StructureContainer, Container),
    (StructureSpawn, Spawn),
    (StructureStorage, Storage),
    (StructureExtension, Extension),
    (StructureLink, Link),
    (StructureTerminal, Terminal),
    (StructureTower, Tower),
    (StructureRoad, Road),
    (StructureWall, Wall),
    (StructureRampart, Rampart),
    (StructureExtractor, Extractor),
    (StructureLab, Lab),
    (StructureNuker, Nuker),
    (StructureFactory, Factory),
    (StructureObserver, Observer),
    (StructurePowerSpawn, PowerSpawn),
);