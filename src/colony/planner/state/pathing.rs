use screeps::{CostMatrix, CostMatrixSet, FindPathOptions, Path, Position, RoomCoordinate, RoomXY, Source, Terrain, pathfinder::SingleRoomCostResult};
use anyhow::anyhow;

use crate::colony::{planner::state::ColonyPlanner, steps::ColonyStep};

#[derive(Clone, Copy)]
pub enum TilePathing {
    BuiltRoad,
    PlannedRoad,
    Plains,
    Swamp,
    Impassable
}

impl From<Terrain> for TilePathing {
    fn from(value: Terrain) -> Self {
        match value {
            Terrain::Plain => TilePathing::Plains,
            Terrain::Swamp => TilePathing::Swamp,
            Terrain::Wall => TilePathing::Impassable,
        }
    }
}

impl TilePathing {
    pub fn cost(self) -> u8 {
        match self {
            TilePathing::BuiltRoad => 5,
            TilePathing::PlannedRoad => 6,
            TilePathing::Plains => 8,
            TilePathing::Swamp => 20,
            TilePathing::Impassable => 255,
        }
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PathId {
    Mineral,
    Source(screeps::ObjectId<Source>)
}

#[derive(Debug)]
pub struct WallConnection {
    pub creep_pos: RoomXY,
    pub road_pos: RoomXY
}

impl ColonyPlanner {
    // Includes start, excludes end
    pub fn find_path_from(&self, xy1: RoomXY, xy2: RoomXY, build_step: Option<ColonyStep>, expiration_step: Option<ColonyStep>) -> Vec<RoomXY> {
        let build_step = build_step.unwrap_or(ColonyStep::last());
        let expiration_step = expiration_step.unwrap_or(ColonyStep::last());

        let mut cost_matrix = self.base_cost_matrix.clone();

        for (pos, timeline) in &self.tiles {
            if let Some(lt) = timeline.road() {
                if lt.is_built_at(build_step) {
                    cost_matrix.set_xy(*pos, TilePathing::BuiltRoad.cost());
                } else if lt.is_planned_at(build_step) {
                    cost_matrix.set_xy(*pos, TilePathing::PlannedRoad.cost());
                }
            }
        }

        for (pos, history) in &self.tiles {
            if history.get_block_step().is_some_and(|block_stop| block_stop < expiration_step) {
                cost_matrix.set_xy(*pos, TilePathing::Impassable.cost());
            }
        }

        let options = FindPathOptions::<fn(_, CostMatrix) -> SingleRoomCostResult, SingleRoomCostResult>::default()
            .cost_callback(|_, _| SingleRoomCostResult::CostMatrix(cost_matrix.clone()))
            .range(1);

        let pos1 = Position::new(xy1.x, xy1.y, self.room.name());
        let pos2 = Position::new(xy2.x, xy2.y, self.room.name());

        let path = pos1.find_path_to(&pos2, Some(options));

        let Path::Vectorized(path) = path else { unreachable!() };

        let mut return_path = vec![xy1];
        return_path.extend(path.into_iter().map(|step|
            RoomXY::new(
                RoomCoordinate::new(step.x as u8).unwrap(),
                RoomCoordinate::new(step.y as u8).unwrap()
            )
        ));

        if return_path.last().is_some_and(|last| *last == xy2) {
            return_path.pop();
        }

        return_path
    }

    pub fn find_wall_connection(&self, center_pos: RoomXY, wall_pos: RoomXY, build_step: Option<ColonyStep>, expiration_step: Option<ColonyStep>) -> anyhow::Result<WallConnection> {
        let path = self.find_path_from(center_pos, wall_pos, build_step, expiration_step);
        let ending = path.last_chunk::<2>().ok_or(anyhow!("Path to wall had too few positions"))?;

        Ok(WallConnection { creep_pos: ending[1], road_pos: ending[0] })
    }
}
