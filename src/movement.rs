use std::collections::HashMap;

use screeps::{Creep, HasPosition, Position};

use crate::safeid::SafeID;

pub enum CreepMovement {
    MoveTo { target: Position, range: u32 },
    TugboatMove { tugged: Creep },
    TuggedMoveTo { tugboat: Creep, target: Position, range: u32 }
}

pub struct MovementSolver(HashMap<SafeID<Creep>, CreepMovement>);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MovementResult {
    InRange, OutOfRange
}

impl MovementResult {
    pub fn in_range(&self) -> bool {
        matches!(self, Self::InRange)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MoveTugboatResult {
    Done, Tugging
}

impl MovementSolver {
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    pub fn move_creep_to(&mut self, creep: &SafeID<Creep>, target: Position, range: u32) -> MovementResult {
        self.0.insert(creep.clone(), CreepMovement::MoveTo { target, range });
        if creep.pos().get_range_to(target) <= range { 
            MovementResult::InRange 
        } else { 
            MovementResult::OutOfRange 
        }
    }

    pub fn move_tugboat(&mut self, tugboat: &SafeID<Creep>, tugged: &Creep) -> MoveTugboatResult {
        self.0.insert(tugboat.clone(), CreepMovement::TugboatMove { tugged: tugged.clone() });
        todo!()
    }

    pub fn move_tugged_to(&mut self, tugged: &SafeID<Creep>, tugboat: &Creep, target: Position, range: u32) -> MovementResult {
        self.0.insert(tugged.clone(), CreepMovement::TuggedMoveTo { tugboat: tugboat.clone(), target, range });
        if tugged.pos().get_range_to(target) <= range { 
            MovementResult::InRange 
        } else { 
            MovementResult::OutOfRange 
        }
    }

    pub fn solve(self) {
        todo!()
    }
}