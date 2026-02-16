use std::collections::{HashMap, HashSet};

use log::*;
use itertools::Itertools;
use screeps::{HasPosition, ObjectId, Position, Room, RoomName, RoomXY, Source, StructureContainer, StructureController, StructureExtension, StructureExtractor, StructureLink, StructureObject, StructureObserver, StructureSpawn, StructureStorage, StructureTerminal, StructureTower, StructureType, find, look};
use serde::{Deserialize, Serialize};
use serde_json_any_key::*;

use crate::colony::{planning::planned_ref::{OptionalPlannedStructureRef, PlannedStructureBuiltRef, PlannedStructureRef}, steps::ColonyStep};

#[derive(Serialize, Deserialize, Clone)]
pub struct ColonyPlan {
    #[serde(with = "any_key_map")]
    pub steps: HashMap<ColonyStep, ColonyPlanStep>,

    pub center: CenterPlan,
    pub mineral: MineralPlan,
    pub sources: SourcesPlan,
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
    pub towers: Vec<PlannedStructureRef<StructureTower>>,
    pub extensions: Vec<PlannedStructureRef<StructureExtension>>
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SourcesPlan(#[serde(with = "any_key_map")] pub HashMap<ObjectId<Source>, SourcePlan>);

#[derive(Serialize, Deserialize, Clone)]
pub struct SourcePlan {
    pub spawn: OptionalPlannedStructureRef<StructureSpawn>,
    pub container: OptionalPlannedStructureRef<StructureContainer>,
    pub link: OptionalPlannedStructureRef<StructureLink>,
    pub extensions: Vec<PlannedStructureRef<StructureExtension>>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct MineralPlan {
    pub container: OptionalPlannedStructureRef<StructureContainer>,
    pub extractor: OptionalPlannedStructureRef<StructureExtractor>
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct ColonyPlanStep {
    pub new_roads: HashSet<RoomXY>,
    #[serde(with = "any_key_map")] 
    pub new_structures: HashMap<RoomXY, StructureType>
}

impl ColonyPlan {
    pub fn diff_with(&self, room: &Room) -> ColonyPlanDiff {
        let planned_roads: HashSet<_> = self.steps.values()
            .flat_map(|step| step.new_roads.iter().cloned())
            .collect();
        let all_roads: HashSet<_> = get_all_roads_in(&room).into_keys().collect();

        let missing_roads = planned_roads.difference(&all_roads)
            .map(|pos| (pos.clone(), RoadDiff::Missing));

        let extra_roads = all_roads.difference(&planned_roads)
            .map(|pos| (pos.clone(), RoadDiff::Extra));

        let road_diff: HashMap<_, _> = missing_roads.chain(extra_roads).collect();

        let planned_structures: HashMap<_, _> = self.steps.values()
            .flat_map(|step| step.new_structures.iter().map(|(a, b)| (a.clone(), b.clone())))
            .collect();
        let all_structures = get_all_structures_in(&room);

        let planned_structure_positions: HashSet<_> = planned_structures.keys().cloned().collect();
        let all_structure_positions: HashSet<_> = all_structures.keys().cloned().collect();

        let missing_structures = planned_structure_positions.difference(&all_structure_positions)
            .map(|pos| (pos.clone(), StructureDiff::Missing(planned_structures[pos])));

        let extra_structures = all_structure_positions.difference(&planned_structure_positions)
            .map(|pos| (pos.clone(), all_structures[pos].0))
            .filter(|(_, structure)| *structure != StructureType::Controller)
            .map(|(pos, structure)| (pos, StructureDiff::Extra(structure)));

        let different_structures = all_structure_positions.intersection(&planned_structure_positions)
            .map(|pos| (pos.clone(), planned_structures[pos], all_structures[pos].0))
            .filter(|(_, expected, found)| *expected != *found)
            .map(|(pos, expected, found)| (pos.clone(), StructureDiff::Different { expected, found }));
        
        let structure_diff: HashMap<_, _> = missing_structures.chain(extra_structures).chain(different_structures).collect();

        ColonyPlanDiff { roads: road_diff, structures: structure_diff }
    }
}

impl ColonyPlanStep {
    pub fn build(&self, room: Room) -> Result<bool, ()> {
        let roads = get_all_roads_in(&room);
        let roads_set: HashSet<_> = roads.keys().cloned().collect();
        let missing_roads = self.new_roads.difference(&roads_set).cloned().collect_vec();

        for road in &missing_roads {
            Position::new(road.x, road.y, room.name()).create_construction_site(StructureType::Road, None).map_err(|_| ())?;
        }

        let all_structures = get_all_structures_in(&room);
        let good_structures: HashSet<_> = all_structures.iter()
            .map(|(a, b)| (*a, *b))
            .filter(|(pos, (ty, _))| 
                self.new_structures.get(pos).map_or(false, |new_ty| *ty == *new_ty)
            ).map(|(pos, _)| pos)
            .collect();

        let missing_structures: HashMap<_, _> = self.new_structures.iter()
            .map(|(a, b)| (*a, *b))
            .filter(|(pos, _)| !good_structures.contains(pos))
            .collect();

        let missing_structure_keys: HashSet<_> = missing_structures.keys().cloned().collect();
        let all_structure_keys: HashSet<_> = all_structures.keys().cloned().collect();
        let overlap = all_structure_keys.intersection(&missing_structure_keys).collect_vec();

        if !overlap.is_empty() {
            warn!("Found structure overlap in {}:", room.name());
            for pos in overlap {
                warn!("For {:?} at {pos}", missing_structures[pos]);
            }

            return Err(())
        }

        for (pos, ty) in &missing_structures {
            Position::new(pos.x, pos.y, room.name()).create_construction_site(*ty, None).map_err(|_| ())?;
        }

        let has_finished_roads = self.new_roads.iter().all(|new_road| roads.get(new_road).cloned().unwrap_or(false));
        let has_finished_structures = self.new_structures.iter().all(|(new_structure, _)| all_structures.get(new_structure).map(|(a, b)| b).cloned().unwrap_or(false));
        Ok(has_finished_roads && has_finished_structures)
    }
}

fn get_all_roads_in(room: &Room) -> HashMap<RoomXY, bool> {
    let built_roads = room.find(find::STRUCTURES, None).into_iter()
        .flat_map(|structure| if let StructureObject::StructureRoad(road) = structure { Some(road) } else { None })
        .map(|road| (road.pos().xy(), true));

    let constructing_roads = room.find(find::MY_CONSTRUCTION_SITES, None).into_iter()
        .filter(|site| matches!(site.structure_type(), StructureType::Road))
        .map(|site| (site.pos().xy(), false));

    built_roads.chain(constructing_roads).collect()
}

fn get_all_structures_in(room: &Room) -> HashMap<RoomXY, (StructureType, bool)> {
    let all_built_structures = room.find(find::STRUCTURES, None).into_iter()
        .filter(|structure| structure.as_owned().map_or(false, |owned| owned.my()) || matches!(structure.structure_type(), StructureType::Container | StructureType::Wall))
        .map(|structure| (structure.pos().xy(), (structure.structure_type(), true)));

    let all_constructing_structures = room.find(find::CONSTRUCTION_SITES, None).into_iter()
        .filter(|site| site.my() || matches!(site.structure_type(), StructureType::Container | StructureType::Wall))
        .map(|site| (site.pos().xy(), (site.structure_type(), false)));

    all_built_structures
        .chain(all_constructing_structures)
        .filter(|(_, (ty, _))| *ty != StructureType::Road)
        .collect()
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RoadDiff {
    Missing,
    Extra
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StructureDiff {
    Missing(StructureType),
    Extra(StructureType),
    Different { expected: StructureType, found: StructureType }
}

pub struct ColonyPlanDiff {
    pub roads: HashMap<RoomXY, RoadDiff>,
    pub structures: HashMap<RoomXY, StructureDiff>
}

impl ColonyPlanDiff {
    pub fn compatible(&self) -> bool {
        self.roads.values().all(|diff| *diff == RoadDiff::Missing) &&
        self.structures.values().all(|diff| matches!(*diff, StructureDiff::Missing(_)))
    }

    pub fn get_removal_losses(&self) -> HashMap<RoomXY, u32> {
        let road_losses = self.roads.iter()
            .filter(|(_, diff)| matches!(diff, RoadDiff::Extra))
            .map(|(pos, _)| (pos.clone(), StructureType::Road.construction_cost().unwrap()));

        let structure_losses = self.structures.iter()
            .flat_map(|(pos, diff)| {
                match diff {
                    StructureDiff::Missing(_) => return None,
                    StructureDiff::Extra(found) |
                    StructureDiff::Different { expected: _, found } => Some((pos.clone(), found.clone()))
                }
            })
            .map(|(pos, structure)| {
                if matches!(structure, StructureType::Rampart | StructureType::Wall) {
                    (pos.clone(), 300000000)
                } else {
                    (pos.clone(), structure.construction_cost().unwrap_or(0))
                }
            });

        road_losses.chain(structure_losses).into_grouping_map().sum()
    }

    pub fn migrate(self, room: RoomName) {
        let road_removals = self.roads.iter()
            .filter(|(_, diff)| matches!(diff, RoadDiff::Extra))
            .map(|(pos, _)| (pos.clone(), StructureType::Road));

        let structure_removals = self.structures.iter()
            .flat_map(|(pos, diff)| {
                match diff {
                    StructureDiff::Missing(_) => return None,
                    StructureDiff::Extra(found) |
                    StructureDiff::Different { expected: _, found } => Some((pos.clone(), found.clone()))
                }
            });

        for (pos, ty) in road_removals.chain(structure_removals) {
            let pos = Position::new(pos.x, pos.y, room);

            let structure = pos.look_for(look::STRUCTURES).unwrap().into_iter()
                .filter(|structure| structure.structure_type() == ty)
                .next();
            if let Some(structure) = structure { structure.as_structure().destroy().ok(); }

            let site = pos.look_for(look::CONSTRUCTION_SITES).unwrap().into_iter()
                .filter(|site| site.structure_type() == ty)
                .next();
            if let Some(site) = site { site.remove().ok(); }
        }
    }
}