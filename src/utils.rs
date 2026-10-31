use itertools::Itertools;
use screeps::{Position, RoomTerrain, RoomXY, Terrain};


pub fn adjacent_positions(pos: Position) -> impl Iterator<Item = Position> {
    (-1..=1).cartesian_product(-1..=1)
        .filter(|(x, y)| !(*x == 0 && *y == 0))
        .map(move |offset| pos + offset)
}

pub trait GetXYGood {
    fn get_xy_good(&self, xy: RoomXY) -> Terrain;
}

impl GetXYGood for RoomTerrain {
    fn get_xy_good(&self, xy: RoomXY) -> Terrain {
        self.get(xy.x.u8(), xy.y.u8())
    }
}