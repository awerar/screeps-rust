use screeps::{Source, StructureType};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Clone, Copy, Debug)]
pub enum PlannedImpassableStructure {
    MainSpawn,
    SourceSpawn(screeps::ObjectId<Source>),
    SourceLink(screeps::ObjectId<Source>),
    SourceExtension(screeps::ObjectId<Source>),
    Extension,
    Storage,
    Terminal,
    Tower,
    CentralLink,
    Extractor,
    Observer
}

impl PlannedImpassableStructure {
    fn buildable_on_wall(&self) -> bool {
        use PlannedImpassableStructure::*;

        matches!(self, Extractor)
    }

    pub fn structure_type(&self) -> StructureType {
        use StructureType::*;

        match self {
            PlannedImpassableStructure::MainSpawn
            | PlannedImpassableStructure::SourceSpawn(_) => Spawn,
            PlannedImpassableStructure::Extension
            | PlannedImpassableStructure::SourceExtension(_) => Extension,
            PlannedImpassableStructure::CentralLink
            | PlannedImpassableStructure::SourceLink(_) => Link,
            PlannedImpassableStructure::Storage => Storage,
            PlannedImpassableStructure::Tower => Tower,
            PlannedImpassableStructure::Terminal => Terminal,
            PlannedImpassableStructure::Extractor => Extractor,
            PlannedImpassableStructure::Observer => Observer
        }
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Clone, Copy, Debug)]
pub enum PlannedContainer {
    Container,
    Source(screeps::ObjectId<Source>),
    Mineral,
    Upgrader
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Clone, Copy, Debug)]
pub enum PlannedStructure {
    Container(PlannedContainer),
    Impassable(PlannedImpassableStructure)
}

impl From<PlannedImpassableStructure> for PlannedStructure { fn from(value: PlannedImpassableStructure) -> Self { Self::Impassable(value) } }
impl From<PlannedContainer> for PlannedStructure { fn from(value: PlannedContainer) -> Self { Self::Container(value) } }

impl PlannedStructure {
    #[expect(unused)]
    fn structure_type(&self) -> StructureType {
        match self {
            PlannedStructure::Container(structure) => StructureType::Container,
            PlannedStructure::Impassable(structure) => structure.structure_type(),
        }
    }
}

impl From<PlannedStructure> for PlannedEntity {
    fn from(value: PlannedStructure) -> Self {
        match value {
            PlannedStructure::Container(structure) => PlannedEntity::Container(structure),
            PlannedStructure::Impassable(structure) => PlannedEntity::Blocker(structure.into()),
        }
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Clone, Copy, Debug)]
pub enum PlannedCreep {
    Upgrader,
    SourceHarvester(screeps::ObjectId<Source>),
    MineralHarvester
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Clone, Copy, Debug)]
pub enum PlannedBlocker {
    Creep(PlannedCreep),
    Structure(PlannedImpassableStructure)
}

impl PlannedBlocker {
    pub fn is_creep(&self) -> bool {
        matches!(self, Self::Creep(_))
    }

    pub fn structure_type(&self) -> Option<StructureType> {
        match self {
            PlannedBlocker::Creep(_) => None,
            PlannedBlocker::Structure(structure) => Some(structure.structure_type()),
        }
    }

    fn plannable_on_wall(&self) -> bool {
        match self {
            PlannedBlocker::Creep(_) => false,
            PlannedBlocker::Structure(structure) => structure.buildable_on_wall(),
        }
    }
}

impl From<PlannedImpassableStructure> for PlannedBlocker { fn from(value: PlannedImpassableStructure) -> Self { Self::Structure(value) } }
impl From<PlannedCreep> for PlannedBlocker { fn from(value: PlannedCreep) -> Self { Self::Creep(value) } }

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Clone, Copy, Debug)]
pub enum PlannedEntity {
    Blocker(PlannedBlocker),
    Container(PlannedContainer)
}

impl PlannedEntity {
    pub fn structure_type(&self) -> Option<StructureType> {
        match self {
            PlannedEntity::Blocker(blocker) => blocker.structure_type(),
            PlannedEntity::Container(_) => Some(StructureType::Container),
        }
    }

    pub fn plannable_on_wall(&self) -> bool {
        match self {
            PlannedEntity::Blocker(blocker) => blocker.plannable_on_wall(),
            PlannedEntity::Container(_) => false,
        }
    }
}

impl From<PlannedBlocker> for PlannedEntity { fn from(value: PlannedBlocker) -> Self { Self::Blocker(value) }}
impl From<PlannedContainer> for PlannedEntity { fn from(value: PlannedContainer) -> Self { Self::Container(value) }}
impl From<PlannedImpassableStructure> for PlannedEntity { fn from(value: PlannedImpassableStructure) -> Self { Self::Blocker(PlannedBlocker::Structure(value)) }}
impl From<PlannedCreep> for PlannedEntity { fn from(value: PlannedCreep) -> Self { Self::Blocker(PlannedBlocker::Creep(value)) }}
