use std::collections::{HashMap, HashSet};

use log::*;
use itertools::Itertools;
use screeps::{HasPosition, ObjectId, Position, Room, RoomXY, Source, StructureContainer, StructureExtension, StructureExtractor, StructureLink, StructureObject, StructureObserver, StructureSpawn, StructureStorage, StructureTerminal, StructureTower, StructureType, find};
use serde::{Deserialize, Serialize};

use crate::colony::{planning::planned_ref::PlannedStructureRef, steps::ColonyStep};

#[derive(Serialize, Deserialize, Clone)]
pub struct ColonyPlan {
    pub steps: HashMap<ColonyStep, ColonyPlanStep>,

    pub center: CenterPlan,
    pub mineral: MineralPlan,
    pub sources: SourcesPlan
}

#[derive(Serialize, Deserialize, Clone)]
pub struct CenterPlan {
    pub pos: Position,

    pub spawn: PlannedStructureRef<StructureSpawn>,
    pub storage: PlannedStructureRef<StructureStorage>,
    pub container_storage: PlannedStructureRef<StructureContainer>,
    pub link: PlannedStructureRef<StructureLink>,
    pub terminal: PlannedStructureRef<StructureTerminal>,
    pub observer: PlannedStructureRef<StructureObserver>,
    pub towers: Vec<PlannedStructureRef<StructureTower>>,
    pub extensions: Vec<PlannedStructureRef<StructureExtension>>
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SourcesPlan(pub HashMap<ObjectId<Source>, SourcePlan>);

#[derive(Serialize, Deserialize, Clone)]
pub struct SourcePlan {
    pub spawn: PlannedStructureRef<StructureSpawn>,
    pub container: PlannedStructureRef<StructureContainer>,
    pub link: PlannedStructureRef<StructureLink>,
    pub extensions: Vec<PlannedStructureRef<StructureExtension>>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct MineralPlan {
    pub container: PlannedStructureRef<StructureContainer>,
    pub extractor: PlannedStructureRef<StructureExtractor>
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct ColonyPlanStep {
    pub new_roads: HashSet<RoomXY>,
    pub new_structures: HashMap<RoomXY, StructureType>
}

impl ColonyPlanStep {
    pub fn build(&self, room: Room) -> Result<bool, ()> {
        let built_roads = room.find(find::STRUCTURES, None).into_iter()
            .flat_map(|structure| if let StructureObject::StructureRoad(road) = structure { Some(road) } else { None })
            .map(|road| road.pos().xy());

        let constructing_roads = room.find(find::MY_CONSTRUCTION_SITES, None).into_iter()
            .filter(|site| matches!(site.structure_type(), StructureType::Road))
            .map(|site| site.pos().xy());

        let roads = built_roads.chain(constructing_roads).collect();
        let missing_roads = self.new_roads.difference(&roads).cloned().collect_vec();

        for road in &missing_roads {
            Position::new(road.x, road.y, room.name()).create_construction_site(StructureType::Road, None).map_err(|_| ())?;
        }

        let all_built_structures = room.find(find::MY_STRUCTURES, None).into_iter()
            .map(|structure| (structure.pos().xy(), structure.structure_type()));

        let all_constructing_structures = room.find(find::MY_CONSTRUCTION_SITES, None).into_iter()
            .map(|site| (site.pos().xy(), site.structure_type()));

        let all_structures: HashMap<_, _> = all_built_structures
            .chain(all_constructing_structures)
            .filter(|(_, ty)| *ty != StructureType::Road)
            .collect();

        let good_structures: HashSet<_> = all_structures.iter()
            .map(|(a, b)| (*a, *b))
            .filter(|(pos, ty)| 
                self.new_structures.get(pos).map_or(false, |new_ty| *ty == *new_ty)
            ).map(|(pos, _)| pos)
            .collect();

        let missing_structures: HashMap<_, _> = self.new_structures.iter()
            .map(|(a, b)| (*a, *b))
            .filter(|(pos, _)| !good_structures.contains(pos))
            .collect();

        let missing_structure_keys: HashSet<_> = missing_structures.keys().cloned().collect();
        let all_structure_keys: HashSet<_> = all_structures.keys().cloned().collect();
        let overlap = all_structure_keys.union(&missing_structure_keys).collect_vec();

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

        Ok(missing_roads.len() == 0 && missing_structures.len() == 0)
    }
}