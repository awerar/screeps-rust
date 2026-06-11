use screeps::Position;
use serde::{Deserialize, Serialize};

pub mod requests;
mod simplifier;
mod solver;
mod solution;

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
struct MoveTarget {
    pub target: Position, 
    pub range: u32
}

impl MoveTarget {
    pub fn in_range(&self, pos: Position) -> bool {
        pos.get_range_to(self.target) <= self.range
    }
}