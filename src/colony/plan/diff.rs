use std::collections::{HashMap, HashSet};

use itertools::Itertools;
use screeps::{Room, RoomXY, StructureType};

use crate::colony::plan::{ColonyPlan, get_all_roads_in, get_all_structures_in};

impl ColonyPlan {
    pub fn diff_with(&self, room: &Room) -> ColonyPlanDiff {
        let planned_roads: HashSet<_> = self.steps.values()
            .flat_map(|step| step.new_roads.iter().copied())
            .collect();
        let all_roads: HashSet<_> = get_all_roads_in(room).into_keys().collect();

        let missing_roads = planned_roads.difference(&all_roads)
            .map(|pos| (*pos, RoadDiff::Missing));

        let extra_roads = all_roads.difference(&planned_roads)
            .map(|pos| (*pos, RoadDiff::Extra));

        let road_diff: HashMap<_, _> = missing_roads.chain(extra_roads).collect();

        let planned_structures: HashMap<_, _> = self.steps.values()
            .flat_map(|step| step.new_structures.iter().map(|(a, b)| (*a, *b)))
            .collect();
        let all_structures = get_all_structures_in(room);

        let planned_structure_positions: HashSet<_> = planned_structures.keys().copied().collect();
        let all_structure_positions: HashSet<_> = all_structures.keys().copied().collect();

        let missing_structures = planned_structure_positions.difference(&all_structure_positions)
            .map(|pos| (*pos, StructureDiff::Missing(planned_structures[pos])));

        let extra_structures = all_structure_positions.difference(&planned_structure_positions)
            .map(|pos| (*pos, all_structures[pos].0))
            .filter(|(_, structure)| *structure != StructureType::Controller)
            .map(|(pos, structure)| (pos, StructureDiff::Extra(structure)));

        let different_structures = all_structure_positions.intersection(&planned_structure_positions)
            .map(|pos| (*pos, planned_structures[pos], all_structures[pos].0))
            .filter(|(_, expected, found)| *expected != *found)
            .map(|(pos, expected, found)| (pos, StructureDiff::Different { expected, found }));

        let structure_diff: HashMap<_, _> = missing_structures.chain(extra_structures).chain(different_structures).collect();

        ColonyPlanDiff { roads: road_diff, structures: structure_diff }
    }
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
            .map(|(pos, _)| (*pos, StructureType::Road.construction_cost().unwrap()));

        let structure_losses = self.structures.iter()
            .filter_map(|(pos, diff)| {
                match diff {
                    StructureDiff::Missing(_) => None,
                    StructureDiff::Extra(found) |
                    StructureDiff::Different { expected: _, found } => Some((*pos, *found))
                }
            })
            .map(|(pos, structure)| {
                if matches!(structure, StructureType::Rampart | StructureType::Wall) {
                    (pos, 300_000_000)
                } else {
                    (pos, structure.construction_cost().unwrap_or(0))
                }
            });

        road_losses.chain(structure_losses).into_grouping_map().sum()
    }
}
