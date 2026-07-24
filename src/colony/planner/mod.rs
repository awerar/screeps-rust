use screeps::{Direction, HasPosition, Room, find};
use anyhow::anyhow;

use crate::colony::{plan::ColonyPlan, planner::{center::{CenterPlanner, find_center, plan_extensions_towers_observer}, connectivity::ensure_connectivity, sources::plan_sources, state::{ColonyPlanner, PlannedStructure}}, steps::ColonyStep};

mod center;
mod connectivity;
mod floodfill;
mod sources;
mod state;

impl ColonyPlan {
    pub fn create_for(room: &Room) -> anyhow::Result<Self> {
        use ColonyStep::*;

        let mut planner = ColonyPlanner::new(room.clone());
        let center = find_center(room);
        planner.plan_structure(center + Direction::Right, BuildBufferAndSourceContainers, PlannedStructure::ContainerStorage)?;

        let mut center_planner = CenterPlanner::new(&planner, center);

        center_planner.plan_structure(&mut planner, BuildLvl4, PlannedStructure::Storage)?;
        center_planner.plan_structure(&mut planner, BuildSpawn, PlannedStructure::MainSpawn)?;
        center_planner.plan_structure(&mut planner, BuildLvl5, PlannedStructure::CentralLink)?;
        center_planner.plan_structure(&mut planner, BuildLvl6, PlannedStructure::Terminal)?;
        center_planner.plan_structure(&mut planner, BuildLvl3, PlannedStructure::Tower)?;

        let excavator_positions = plan_sources(&mut planner, center)?;

        plan_extensions_towers_observer(&mut planner, &mut center_planner)?;

        center_planner.plan_roads(&mut planner);

        let controller = room.controller().unwrap().pos().xy();
        planner.plan_road_between(center, controller, BuildArterialRoads);

        for source in excavator_positions {
            planner.plan_road_between(source, center, BuildArterialRoads);
        }

        for deposit in room.find(find::MINERALS, None) {
            planner.plan_structure(deposit.pos().xy(), BuildLvl6, PlannedStructure::Extractor)?;
            planner.plan_road_between(center, deposit.pos().xy(), BuildLvl6);

            let container_pos = deposit.pos().xy().neighbors().into_iter()
                .find(|neigh| planner.roads.contains_key(neigh))
                .ok_or(anyhow!("Unable to find road around deposit"))?;
            planner.plan_structure(container_pos, BuildLvl6, PlannedStructure::MineralContainer)?;
        }

        ensure_connectivity(&mut planner, center)?;

        planner.compile()
    }
}
