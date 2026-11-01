use screeps::{HasPosition, Room, find};

use crate::colony::{plan::ColonyPlan, planner::{center::{CenterPlanner, find_center, plan_extensions_towers_observer}, sources::plan_sources, state::{ColonyPlanner, LifeTime, PathId, PlannedContainer, PlannedCreep, PlannedImpassableStructure}}, steps::ColonyStep};

mod center;
mod floodfill;
mod sources;
mod state;
mod block_checker;

impl ColonyPlan {
    pub fn create_for(room: &Room) -> anyhow::Result<Self> {
        use ColonyStep::*;

        let mut planner = ColonyPlanner::new(room.clone());
        let center = find_center(room)?;
        
        let mut center_planner = CenterPlanner::new(center);

        center_planner.plan_container_buffer(&mut planner, LifeTime::temporary(BuildBufferAndSourceContainers, UpgradeToLevel5).unwrap())?;
        center_planner.plan_structure(&mut planner, BuildLvl4, PlannedImpassableStructure::Storage)?;
        center_planner.plan_structure(&mut planner, BuildSpawn, PlannedImpassableStructure::MainSpawn)?;
        center_planner.plan_structure(&mut planner, BuildLvl5, PlannedImpassableStructure::CentralLink)?;
        center_planner.plan_structure(&mut planner, BuildLvl6, PlannedImpassableStructure::Terminal)?;
        center_planner.plan_structure(&mut planner, BuildLvl3, PlannedImpassableStructure::Tower)?;

        let source_datas = plan_sources(&mut planner, center)?;

        let controller = room.controller().unwrap().pos().xy();
        let controller_connection = planner.find_wall_connection(center, controller, Some(BuildArterialRoads), None)?;
        planner.plan_blocker(controller_connection.creep_pos, ColonyStep::BuildSpawn, PlannedCreep::Upgrader.into())?;

        let deposits = room.find(find::MINERALS, None);
        assert!(deposits.len() <= 1);
        let deposit = deposits.into_iter().next().map(|deposit| deposit.pos().xy());
        let deposit_connection = if let Some(deposit) = deposit {
            let connection = planner.find_wall_connection(center, deposit, Some(BuildLvl6), None)?;

            planner.plan_blocker(deposit, BuildLvl6, PlannedImpassableStructure::Extractor.into())?;
            planner.plan_container(connection.creep_pos, LifeTime::permanent(BuildLvl6), PlannedContainer::MineralContainer)?;

            Some(connection)
        } else { None };

        plan_extensions_towers_observer(&mut planner, &mut center_planner)?;
        center_planner.plan_roads(&mut planner);

        planner.plan_road_from(controller_connection.access_pos, center, LifeTime::permanent(BuildArterialRoads), None)?;

        for source_data in source_datas {
            planner.plan_road_from(
                source_data.access_pos, 
                center,
                LifeTime::temporary(BuildArterialRoads, source_data.link_step).unwrap(),
                Some(PathId::Source(source_data.source))
            )?;
        }

        if let Some(connection) = deposit_connection {
            planner.plan_road_from(connection.access_pos, center, LifeTime::permanent(BuildLvl6), Some(PathId::Mineral))?;
        }

        planner.compile()
    }
}
