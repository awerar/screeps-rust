use itertools::Itertools;
use screeps::{HasId, HasPosition, RoomCoordinate, RoomXY, find};
use anyhow::anyhow;

use crate::colony::{planner::state::{ColonyPlanner, PlannedPermanentStructure}, steps::ColonyStep};

pub fn plan_sources(planner: &mut ColonyPlanner, center: RoomXY) -> anyhow::Result<Vec<RoomXY>> {
    use ColonyStep::*;

    let mut connection_points = Vec::new();
    for source in planner.room.find(find::SOURCES, None).into_iter().sorted_by_key(screeps::HasId::id) {
        let source_pos = source.pos().xy();
        let source_id = source.id();

        let path = planner.find_path_between(source_pos, center, Some(BuildArterialRoads));

        let harvest_pos = path.first().ok_or(anyhow!("Path to source had zero elements"))?;
        let excavator_pos = RoomXY::new(
            RoomCoordinate::new(harvest_pos.x as u8).unwrap(),
            RoomCoordinate::new(harvest_pos.y as u8).unwrap()
        );

        planner.plan_road(excavator_pos, BuildArterialRoads);
        planner.plan_blocker(excavator_pos, BuildBufferAndSourceContainers, PlannedPermanentStructure::SourceContainer(source_id))?;

        let slots = excavator_pos.neighbors().into_iter()
            .filter(|neigh| planner.is_free_at(*neigh))
            .collect_vec()
            .into_iter();

        let main_road_pos = path.get(1).ok_or(anyhow!("Path to source had one element"))?;
        let main_road_pos = RoomXY::new(
            RoomCoordinate::new(main_road_pos.x as u8).unwrap(),
            RoomCoordinate::new(main_road_pos.y as u8).unwrap()
        );

        planner.plan_road(main_road_pos, BuildArterialRoads);
        planner.plan_blocker_earliest(main_road_pos, PlannedPermanentStructure::SourceSpawn(source_id))?;

        let mut slots = slots.filter(|slot| *slot != main_road_pos);
        let link_slot = slots.next().ok_or(anyhow!("No slots for link around source"))?;
        planner.plan_blocker_earliest(link_slot, PlannedPermanentStructure::SourceLink(source_id))?;

        for slot in slots {
            planner.plan_blocker_earliest(slot, PlannedPermanentStructure::SourceExtension(source_id))?;
        }

        connection_points.push(main_road_pos);
    }

    Ok(connection_points)
}
