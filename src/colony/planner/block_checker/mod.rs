use std::{cell::RefCell, collections::HashSet};

use anyhow::bail;
use screeps::{Room, RoomXY};

use crate::colony::planner::block_checker::wall_union::WallUnionChecker;

mod wall_union;

// Also add a checker that makes sure each non-blocked tile is connected
// and each building should be connected to a non-blocked tile
pub struct BlockChecker {
    blocked: HashSet<RoomXY>,
    wall_union: RefCell<WallUnionChecker>
}

impl BlockChecker {
    pub fn new(room: &Room) -> Self {
        BlockChecker { 
            wall_union: RefCell::new(WallUnionChecker::new(room)),
            blocked: HashSet::new()
        }
    }

    pub fn can_block(&self, xy: RoomXY) -> bool {
        !self.blocked.contains(&xy) &&
        self.wall_union.borrow_mut().can_block(xy)
    }

    pub fn block(&mut self, xy: RoomXY) -> anyhow::Result<()> {
        if self.blocked.contains(&xy) { bail!("Already blocked at {xy}") }
        self.blocked.insert(xy);

        self.wall_union.borrow_mut().block(xy)
    }

    pub fn has_blocked_at(&mut self, xy: RoomXY) -> bool {
        self.blocked.contains(&xy)
    }
}