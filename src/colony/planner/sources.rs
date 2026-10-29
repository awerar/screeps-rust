use itertools::Itertools;
use screeps::{HasId, HasPosition, RoomXY, Source, StructureType, find};
use anyhow::anyhow;

use crate::colony::{planner::state::{ColonyPlanner, PlannedPermanentStructure}, steps::ColonyStep};

pub struct SourceData {
    pub connection_point: RoomXY,
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
        let path = planner.find_path_between(source_pos, center, Some(BuildArterialRoads), Some(link_step));

        let excavator_pos = *path.first().ok_or(anyhow!("Path to source had zero elements"))?;
        let main_road_pos = *path.get(1).ok_or(anyhow!("Path to source had one element"))?;

        planner.plan_blocker(excavator_pos, BuildBufferAndSourceContainers, PlannedPermanentStructure::SourceContainer(source_id).into())?;

        let slots = excavator_pos.neighbors().into_iter()
            .filter(|neigh| planner.is_free_at(*neigh))
            .collect_vec()
            .into_iter();


        planner.plan_blocker_earliest(main_road_pos, PlannedPermanentStructure::SourceSpawn(source_id).into())?;

        let mut slots = slots.filter(|slot| *slot != main_road_pos);
        let link_slot = slots.next().ok_or(anyhow!("No slots for link around source"))?;
        planner.plan_blocker(link_slot, link_step, PlannedPermanentStructure::SourceLink(source_id).into())?;

        for slot in slots {
            planner.plan_blocker_earliest(slot, PlannedPermanentStructure::SourceExtension(source_id).into())?;
        }

        source_datas.push(
            SourceData { 
                connection_point: main_road_pos, 
                link_step,
                source: source.id()
            }
        );
    }

    Ok(source_datas)
}
