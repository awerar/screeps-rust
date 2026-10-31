use std::cell::RefCell;

use screeps::{Room, RoomXY};

use crate::colony::planner::block_checker::wall_union::WallUnionChecker;

mod wall_union;

pub struct BlockChecker {
    wall_union: RefCell<WallUnionChecker>
}

impl BlockChecker {
    pub fn new(room: &Room) -> Self {
        BlockChecker { 
            wall_union: RefCell::new(WallUnionChecker::new(room))
        }
    }

    pub fn can_block(&self, xy: RoomXY) -> bool {
        self.wall_union.borrow_mut().can_block(xy)
    }

    pub fn block(&mut self, xy: RoomXY) -> anyhow::Result<()> {
        self.wall_union.borrow_mut().block(xy)
    }
}