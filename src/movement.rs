use std::collections::HashMap;

use screeps::{Creep, HasPosition, Position};

use crate::safeid::{GetSafeID, SafeID};

pub enum CreepMovement {
    MoveTo { target: Position, range: u32 },
    TugboatMove { tugged: Creep },
    TuggedMoveTo { tugboat: Creep, target: Position, range: u32 }
}

pub struct MovementSolver(HashMap<SafeID<Creep>, CreepMovement>);

impl MovementSolver {
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    pub fn move_creep_to(&mut self, creep: &Creep, target: Position, range: u32) -> bool {
        self.0.insert(creep.safe_id(), CreepMovement::MoveTo { target, range });
        creep.pos().get_range_to(target) <= range
    }

    pub fn move_tugboat(&mut self, tugboat: &Creep, tugged: &Creep) -> bool {
        self.0.insert(tugboat.safe_id(), CreepMovement::TugboatMove { tugged: tugged.clone() });
        todo!()
    }

    pub fn move_tugged_to(&mut self, tugged: &Creep, tugboat: &Creep, target: Position, range: u32) -> bool {
        self.0.insert(tugged.safe_id(), CreepMovement::TuggedMoveTo { tugboat: tugboat.clone(), target, range });
        tugged.pos().get_range_to(target) <= range
    }

    pub fn solve(self) {
        todo!()
    }
}