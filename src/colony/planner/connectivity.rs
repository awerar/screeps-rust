use log::debug;
use itertools::Itertools;
use screeps::RoomXY;
use strum::IntoEnumIterator;
use unionfind::HashUnionFindByRank;

use crate::colony::{planner::state::{ColonyPlanner, PlannedStructure}, steps::ColonyStep};

pub fn ensure_connectivity(planner: &mut ColonyPlanner, center: RoomXY) -> anyhow::Result<()> {
    let mut network = HashUnionFindByRank::new(vec![center]).unwrap();

    for step in ColonyStep::iter() {
        let new_roads: Vec<_> = planner.roads.iter()
            .filter(|(_, road_step)| step == **road_step)
            .map(|(pos, _)| pos)
            .copied()
            .sorted()
            .collect();

        for new_road in &new_roads {
            network.add(*new_road)?;
            for neigh in new_road.neighbors() {
                if network.find_shorten(&neigh).is_some() {
                    network.union_by_rank(new_road, &neigh)?;
                }
            }
        }

        for new_road in &new_roads {
            if network.find_shorten(new_road) != network.find_shorten(&center) {
                planner.plan_road_between(center, *new_road, step);
                network.union_by_rank(new_road, &center)?;
            }
        }

        let new_structures: Vec<_> = planner.structures.iter()
            .filter(|(_, road_step)| step == **road_step)
            .map(|(pos, _)| pos)
            .filter(|pos| !matches!(planner.pos2structure[*pos], PlannedStructure::SourceContainer(_)))
            .copied()
            .sorted()
            .collect();

        for new_structure in new_structures {
            if new_structure == center { continue; }
            if new_structure.neighbors().into_iter().any(|neigh| network.find_shorten(&neigh).is_some()) { continue; }
            debug!("Connecting {center} and {new_structure}");
            planner.plan_road_between(center, new_structure, step);
        }
    }

    Ok(())
}
