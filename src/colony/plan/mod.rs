use std::collections::{HashMap, HashSet};

use screeps::{Direction, HasPosition, ObjectId, OwnedStructureProperties, Position, Room, RoomXY, Source, StructureContainer, StructureController, StructureExtension, StructureExtractor, StructureLink, StructureObject, StructureObserver, StructureSpawn, StructureStorage, StructureTerminal, StructureTower, StructureType, find};
use serde::{Deserialize, Serialize};
use serde_json_any_key::any_key_map;

use crate::colony::{plan::refs::{OptionalPlannedStructureRef, PlannedStructureBuiltRef, PlannedStructureRef, PlannedStructureRefs}, steps::ColonyStep};

mod diff;
mod execute;
pub mod refs;
mod visuals;

pub use diff::{ColonyPlanDiff, RoadDiff, StructureDiff};

#[derive(Serialize, Deserialize, Clone)]
pub struct ColonyPlan {
    #[serde(with = "any_key_map")]
    pub steps: HashMap<ColonyStep, ColonyPlanStep>,

    #[serde(with = "any_key_map")]
    pub sources: SourcesPlan,
    pub center: CenterPlan,
    pub mineral: MineralPlan,
    pub controller: PlannedStructureBuiltRef<StructureController>
}

#[derive(Serialize, Deserialize, Clone)]
pub struct CenterPlan {
    pub pos: Position,

    pub spawn: PlannedStructureRef<StructureSpawn>,
    pub storage: OptionalPlannedStructureRef<StructureStorage>,
    pub container_storage: OptionalPlannedStructureRef<StructureContainer>,
    pub link: OptionalPlannedStructureRef<StructureLink>,
    pub terminal: OptionalPlannedStructureRef<StructureTerminal>,
    pub observer: OptionalPlannedStructureRef<StructureObserver>,
    pub towers: PlannedStructureRefs<StructureTower>,
    pub extensions: PlannedStructureRefs<StructureExtension>
}

pub type SourcesPlan = HashMap<ObjectId<Source>, SourcePlan>;

#[derive(Serialize, Deserialize, Clone)]
pub struct SourcePlan {
    pub spawn: OptionalPlannedStructureRef<StructureSpawn>,
    pub container: OptionalPlannedStructureRef<StructureContainer>,
    pub link: OptionalPlannedStructureRef<StructureLink>,
    pub extensions: PlannedStructureRefs<StructureExtension>,

    pub distance: u32,
    pub spawn_direction: Direction
}

#[derive(Clone, Serialize, Deserialize)]
pub struct MineralPlan {
    pub container: OptionalPlannedStructureRef<StructureContainer>,
    pub extractor: OptionalPlannedStructureRef<StructureExtractor>,

    pub distance: u32
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct ColonyPlanStep {
    pub new_roads: HashSet<RoomXY>,
    #[serde(with = "any_key_map")]
    pub new_structures: HashMap<RoomXY, StructureType>
}

pub fn get_all_roads_in(room: &Room) -> HashMap<RoomXY, bool> {
    let built_roads = room.find(find::STRUCTURES, None).into_iter()
        .filter_map(|structure| if let StructureObject::StructureRoad(road) = structure { Some(road) } else { None })
        .map(|road| (road.pos().xy(), true));

    let constructing_roads = room.find(find::MY_CONSTRUCTION_SITES, None).into_iter()
        .filter(|site| matches!(site.structure_type(), StructureType::Road))
        .map(|site| (site.pos().xy(), false));

    built_roads.chain(constructing_roads).collect()
}

pub fn get_all_structures_in(room: &Room) -> HashMap<RoomXY, (StructureType, bool)> {
    let all_built_structures = room.find(find::STRUCTURES, None).into_iter()
        .filter(|structure| structure.as_owned().is_some_and(OwnedStructureProperties::my) || matches!(structure.structure_type(), StructureType::Container | StructureType::Wall))
        .map(|structure| (structure.pos().xy(), (structure.structure_type(), true)));

    let all_constructing_structures = room.find(find::CONSTRUCTION_SITES, None).into_iter()
        .filter(|site| site.my() || matches!(site.structure_type(), StructureType::Container | StructureType::Wall))
        .map(|site| (site.pos().xy(), (site.structure_type(), false)));

    all_built_structures
        .chain(all_constructing_structures)
        .filter(|(_, (ty, _))| *ty != StructureType::Road)
        .collect()
}
