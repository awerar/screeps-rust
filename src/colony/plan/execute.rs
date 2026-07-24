use std::collections::{HashMap, HashSet, VecDeque};

use log::warn;
use itertools::Itertools;
use screeps::{Position, Room, RoomName, StructureType, look};
use anyhow::anyhow;
use strum::IntoEnumIterator;

use crate::colony::{plan::{ColonyPlan, ColonyPlanDiff, ColonyPlanStep, RoadDiff, StructureDiff, get_all_roads_in, get_all_structures_in}, steps::ColonyStep};

impl ColonyPlan {
    pub fn adapt_build_times_to(&mut self, room: &Room) {
        let mut structures_left_to_adjust: HashMap<_, VecDeque<_>> = get_all_structures_in(room).into_iter()
            .map(|(pos, (ty, _))| (ty, pos))
            .filter(|(ty, _)| *ty != StructureType::Controller)
            .into_grouping_map()
            .collect();

        let mut adjusted_positions: HashSet<_> = structures_left_to_adjust.values().flatten().copied().collect();

        for step in ColonyStep::iter() {
            let Some(step) = self.steps.get_mut(&step) else { continue; };

            for (pos, ty) in step.new_structures.clone() {
                let left_to_adjust = structures_left_to_adjust.entry(ty).or_default();
                let Some(new_pos) = left_to_adjust.pop_front() else { continue; };

                step.new_structures.remove(&pos);
                step.new_structures.insert(new_pos, ty);

                if !adjusted_positions.contains(&pos) {
                    adjusted_positions.insert(pos);
                    left_to_adjust.push_back(pos);
                }
            }
        }

        assert_eq!(structures_left_to_adjust.iter().flat_map(|(ty, positions)| positions.iter().map(|pos| (*ty, pos))).collect_vec(), vec![]);
    }
}

impl ColonyPlanStep {
    pub fn build(&self, room: &Room) -> anyhow::Result<bool> {
        let roads = get_all_roads_in(room);
        let roads_set: HashSet<_> = roads.keys().copied().collect();
        let missing_roads = self.new_roads.difference(&roads_set).copied().collect_vec();

        for road in &missing_roads {
            Position::new(road.x, road.y, room.name()).create_construction_site(StructureType::Road, None)?;
        }

        let all_structures = get_all_structures_in(room);
        let good_structures: HashSet<_> = all_structures.iter()
            .map(|(a, b)| (*a, *b))
            .filter(|(pos, (ty, _))|
                self.new_structures.get(pos).is_some_and(|new_ty| *ty == *new_ty)
            ).map(|(pos, _)| pos)
            .collect();

        let missing_structures: HashMap<_, _> = self.new_structures.iter()
            .map(|(a, b)| (*a, *b))
            .filter(|(pos, _)| !good_structures.contains(pos))
            .collect();

        let missing_structure_keys: HashSet<_> = missing_structures.keys().copied().collect();
        let all_structure_keys: HashSet<_> = all_structures.keys().copied().collect();
        let overlap = all_structure_keys.intersection(&missing_structure_keys).collect_vec();

        if !overlap.is_empty() {
            warn!("Found structure overlap in {}:", room.name());
            for pos in overlap {
                warn!("For {:?} at {pos}", missing_structures[pos]);
            }

            return Err(anyhow!("Structure overlap"))
        }

        for (pos, ty) in &missing_structures {
            let pos = Position::new(pos.x, pos.y, room.name());
            if pos.look_for(look::CONSTRUCTION_SITES).ok().is_none_or(|sites| sites.is_empty()) {
                pos.create_construction_site(*ty, None).map_err(|e| anyhow!("Unable to create structure {ty} at {pos}: {e}"))?;
            }
        }

        let has_finished_roads = self.new_roads.iter().all(|new_road| roads.get(new_road).copied().unwrap_or(false));
        let has_finished_structures = self.new_structures.iter().all(|(new_structure, _)| all_structures.get(new_structure).map(|(_, b)| b).copied().unwrap_or(false));
        Ok(has_finished_roads && has_finished_structures)
    }
}

impl ColonyPlanDiff {
    pub fn migrate(self, room: RoomName) {
        let road_removals = self.roads.iter()
            .filter(|(_, diff)| matches!(diff, RoadDiff::Extra))
            .map(|(pos, _)| (*pos, StructureType::Road));

        let structure_removals = self.structures.iter()
            .filter_map(|(pos, diff)| {
                match diff {
                    StructureDiff::Missing(_) => None,
                    StructureDiff::Extra(found) |
                    StructureDiff::Different { expected: _, found } => Some((*pos, *found))
                }
            });

        for (pos, ty) in road_removals.chain(structure_removals) {
            let pos = Position::new(pos.x, pos.y, room);

            let structure = pos.look_for(look::STRUCTURES).unwrap().into_iter()
                .find(|structure| structure.structure_type() == ty);
            if let Some(structure) = structure { structure.as_structure().destroy().ok(); }

            let site = pos.look_for(look::CONSTRUCTION_SITES).unwrap().into_iter()
                .find(|site| site.structure_type() == ty);
            if let Some(site) = site { site.remove().ok(); }
        }
    }
}
