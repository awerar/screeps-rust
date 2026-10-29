use std::collections::{HashMap, HashSet};

use itertools::Itertools;
use screeps::{Room, RoomXY, StructureType};

use crate::colony::plan::{ColonyPlan, get_all_structures_in};

impl ColonyPlan {
    pub fn diff_with(&self, room: &Room) -> ColonyPlanDiff {
        let controller = room.controller().expect("ColonyPlan should only be created for rooms with controllers");

        let planned_structures = self.steps.iter()
            .take_while(|(step, _)| step.controller_level() <= controller.level())
            .flat_map(|(_, step)| step.new_structures.iter().map(|(a, b)| (*a, *b)))
            .collect::<HashSet<_>>();

        let all_structures = get_all_structures_in(room).into_keys()
            .collect::<HashSet<_>>();

        ColonyPlanDiff(all_structures.difference(&planned_structures).copied().collect_vec())
    }
}

pub struct ColonyPlanDiff(pub Vec<(RoomXY, StructureType)>);

impl ColonyPlanDiff {
    pub fn compatible(&self) -> bool {
        self.0.is_empty()
    }

    pub fn get_removal_losses(&self) -> HashMap<RoomXY, u32> {
        self.0.iter()
            .map(|(pos, structure)| {
                if matches!(structure, StructureType::Rampart | StructureType::Wall) {
                    (*pos, 300_000_000)
                } else {
                    (*pos, structure.construction_cost().unwrap_or(0))
                }
            }).into_grouping_map().sum()
    }
}
