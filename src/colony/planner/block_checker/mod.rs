use std::{cell::RefCell, collections::HashSet};

use anyhow::bail;
use itertools::Itertools;
use screeps::{Room, RoomTerrain, RoomXY, Terrain};

use crate::{colony::planner::block_checker::wall_union::WallUnionChecker, utils::GetXYGood};

mod wall_union;

pub struct BlockChecker {
    terrain: RoomTerrain,
    blocked: HashSet<RoomXY>,
    wall_union: RefCell<WallUnionChecker>
}

impl BlockChecker {
    pub fn new(room: &Room) -> Self {
        BlockChecker { 
            terrain: room.get_terrain(),
            wall_union: RefCell::new(WallUnionChecker::new(room)),
            blocked: HashSet::new()
        }
    }

    pub fn can_block(&self, xy: RoomXY) -> bool {
        !self.blocked.contains(&xy)
        && self.wall_union.borrow_mut().can_block(xy)
        && !(self.might_disconnect(xy) && self.disconnects(xy))
    }

    pub fn block(&mut self, xy: RoomXY) -> anyhow::Result<()> {
        if self.might_disconnect(xy) && self.disconnects(xy) { bail!("Blocking here would disconnect the room") }
        if self.blocked.contains(&xy) { bail!("Already blocked at {xy}") }

        self.blocked.insert(xy);

        self.wall_union.borrow_mut().block(xy)
    }

    fn is_impassable(&self, xy: RoomXY) -> bool {
        self.terrain.get_xy_good(xy) == Terrain::Wall || self.blocked.contains(&xy)
    }

    fn might_disconnect(&self, xy: RoomXY) -> bool {
        !all_connected(xy.neighbors().into_iter().filter(|xy| !self.is_impassable(*xy)).collect())
    }

    fn disconnects(&self, block_pos: RoomXY) -> bool {
        !all_connected(
            (0..50).cartesian_product(0..50)
                .map(|(x, y)| RoomXY::new(x.try_into().unwrap(), y.try_into().unwrap()))
                .filter(|xy| !self.is_impassable(*xy) && *xy != block_pos)
                .collect()
        )
    }
}

fn all_connected(mut xys: HashSet<RoomXY>) -> bool {
    let Some(seed) = xys.iter().next().copied() else { return true };
    xys.remove(&seed);

    let mut stack = Vec::new();
    stack.push(seed);

    while let Some(xy) = stack.pop() {
        for neigh in xy.neighbors() {
            if xys.remove(&neigh) {
                stack.push(neigh);
            }
        }
    }

    xys.is_empty()
}