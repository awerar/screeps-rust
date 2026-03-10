use std::collections::{HashMap, HashSet};

use screeps::{Creep, HasPosition, Position};
use serde::{Deserialize, Serialize};

use crate::safeid::{SafeID, deserialize_prune_hashset};

#[derive(Serialize, Deserialize, Default)]
pub struct MovementSolver {
    #[serde(deserialize_with = "deserialize_prune_hashset")]
    done_tugboats: HashSet<SafeID<Creep>>,

    #[serde(default, skip)]
    requests: HashMap<SafeID<Creep>, MovementRequest>
}

pub enum MovementRequest {
    MoveTo { target: Position, range: u32 },
    TugboatMove { tugged: SafeID<Creep> },
    TuggedMoveTo { target: Position, range: u32 }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MoveToResult {
    InRange, OutOfRange
}

impl MoveToResult {
    pub fn in_range(self) -> bool {
        matches!(self, Self::InRange)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MoveTugboatResult {
    Done, NotDone
}

impl MovementSolver {
    pub fn move_creep_to(&mut self, creep: &SafeID<Creep>, target: Position, range: u32) -> MoveToResult {
        self.requests.insert(creep.clone(), MovementRequest::MoveTo { target, range });
        if creep.pos().get_range_to(target) <= range { 
            MoveToResult::InRange 
        } else { 
            MoveToResult::OutOfRange 
        }
    }

    pub fn move_tugboat(&mut self, tugboat: &SafeID<Creep>, tugged: &SafeID<Creep>) -> MoveTugboatResult {
        if self.done_tugboats.contains(tugboat) {
            MoveTugboatResult::Done
        } else {
            self.requests.insert(tugboat.clone(), MovementRequest::TugboatMove { tugged: tugged.clone() });
            MoveTugboatResult::NotDone
        }
    }

    pub fn move_tugged_to(&mut self, creep: &SafeID<Creep>, target: Position, range: u32) -> MoveToResult {
        self.requests.insert(creep.clone(), MovementRequest::TuggedMoveTo { target, range });
        if creep.pos().get_range_to(target) <= range { 
            MoveToResult::InRange 
        } else { 
            MoveToResult::OutOfRange 
        }
    }

    pub fn solve(&mut self) {
        todo!()
    }
}