use itertools::Itertools;
use screeps::{HasId, HasPosition, RoomXY, Source, StructureType, find};
use anyhow::anyhow;

use crate::colony::{planner::state::{ColonyPlanner, PlannedPermanentStructure}, steps::ColonyStep};

pub struct SourceData {
    pub access_pos: RoomXY,
    pub link_step: ColonyStep,
    pub source: screeps::ObjectId<Source>
}

pub fn plan_sources(planner: &mut ColonyPlanner, center: RoomXY) -> anyhow::Result<Vec<SourceData>> {
    use ColonyStep::*;

    let mut source_datas = Vec::new();
    for source in planner.room.find(find::SOURCES, None).into_iter().sorted_by_key(screeps::HasId::id) {
        let source_pos = source.pos().xy();
        let source_id = source.id();

        let link_step = planner.get_earliest_step_for(StructureType::Link)?;
        let connection = planner.find_wall_connection(center, source_pos, Some(BuildArterialRoads), Some(link_step))?;

        planner.plan_blocker(connection.creep_pos, BuildBufferAndSourceContainers, PlannedPermanentStructure::SourceContainer(source_id).into())?;

        let slots = connection.creep_pos.neighbors().into_iter()
            .filter(|neigh| planner.can_block_at(*neigh, None))
            .collect_vec()
            .into_iter();


        planner.plan_blocker_earliest(connection.access_pos, PlannedPermanentStructure::SourceSpawn(source_id))?;

        let mut slots = slots.filter(|slot| *slot != connection.access_pos);
        let link_slot = slots.next().ok_or(anyhow!("No slots for link around source"))?;
        planner.plan_blocker(link_slot, link_step, PlannedPermanentStructure::SourceLink(source_id).into())?;

        for slot in slots {
            planner.plan_blocker_earliest(slot, PlannedPermanentStructure::SourceExtension(source_id))?;
        }

        source_datas.push(
            SourceData { 
                access_pos: connection.access_pos, 
                link_step,
                source: source.id()
            }
        );
    }

    Ok(source_datas)
}
