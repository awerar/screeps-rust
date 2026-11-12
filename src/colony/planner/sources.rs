use itertools::Itertools;
use screeps::{HasId, HasPosition, RoomXY, Source, StructureType, find};
use anyhow::anyhow;

use crate::colony::{planner::state::{ColonyPlanner, LifeTime, PlannedContainer, PlannedCreep, PlannedImpassableStructure}, steps::ColonyStep};

pub struct SourceData {
    pub road_pos: RoomXY,
    pub link_step: ColonyStep,
    pub source: screeps::ObjectId<Source>
}

struct SourceSlots(<Vec<RoomXY> as IntoIterator>::IntoIter);

impl SourceSlots {
    fn new(slots: Vec<RoomXY>) -> Self {
        Self(slots.into_iter())
    }

    fn next(&mut self, planner: &ColonyPlanner) -> Option<RoomXY> {
        for xy in self.0.by_ref() {
            if planner.is_free_at(xy) && planner.can_block_at(xy) {
                return Some(xy)
            }
        }

        None
    }
}

pub fn plan_sources(planner: &mut ColonyPlanner, center: RoomXY) -> anyhow::Result<Vec<SourceData>> {
    use ColonyStep::*;

    let mut source_datas = Vec::new();
    for source in planner.room.find(find::SOURCES, None).into_iter().sorted_by_key(screeps::HasId::id) {
        let source_pos = source.pos().xy();
        let source_id = source.id();

        let link_step = planner.get_earliest_step_for(StructureType::Link)?;
        let connection = planner.find_wall_connection(center, source_pos, Some(BuildArterialRoads), Some(link_step))?;

        planner.plan_container(connection.creep_pos, LifeTime::permanent(BuildCenterAndSourceContainers), PlannedContainer::Source(source_id))?;
        planner.plan_blocker(connection.creep_pos, ColonyStep::first(), PlannedCreep::SourceHarvester(source.id()).into())?;
        planner.plan_blocker_earliest(connection.road_pos, PlannedImpassableStructure::SourceSpawn(source_id))?;

        let mut slots = SourceSlots::new(connection.creep_pos.neighbors());

        let link_slot = slots.next(planner).ok_or(anyhow!("No slots for link around source"))?;
        planner.plan_blocker(link_slot, link_step, PlannedImpassableStructure::SourceLink(source_id).into())?;

        while let Some(slot) = slots.next(planner) {
            planner.plan_blocker_earliest(slot, PlannedImpassableStructure::SourceExtension(source_id))?;
        }

        source_datas.push(
            SourceData { 
                road_pos: connection.road_pos, 
                link_step,
                source: source.id()
            }
        );
    }

    Ok(source_datas)
}
