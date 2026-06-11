use std::{collections::{HashMap, HashSet}, assert_matches};

use screeps::{Creep, Direction, HasPosition, Position, RoomName, RoomXY};

use crate::safeid::SafeID;

#[derive(Debug)]
pub enum MoveAction {
    Move { dir: Direction },
    Pulled { next: SafeID<Creep> },
    Stay
}

impl MoveAction {
    fn apply(&self, pos: Position) -> Position {
        match self {
            MoveAction::Move { dir } => pos + *dir,
            MoveAction::Pulled { next } => next.pos(),
            MoveAction::Stay => pos,
        }
    }
}

pub struct MovementSolution {
    next: HashMap<Position, SafeID<Creep>>,
    actions: HashMap<SafeID<Creep>, MoveAction>,
    blocked_positions: HashMap<RoomName, HashSet<RoomXY>>
}

impl MovementSolution {
    pub fn new() -> Self {
        Self {
            actions: HashMap::new(),
            next: HashMap::new(),
            blocked_positions: HashMap::new()
        }
    }

    pub fn give_action_for(&mut self, creep: &SafeID<Creep>, action: MoveAction) {
        let pos = action.apply(creep.pos());

        if let Some(other) = self.next.get(&pos).cloned() {
            if !matches!(action, MoveAction::Stay) {
                self.give_action_for(creep, MoveAction::Stay);
                return;
            }

            self.cancel_action_for(&other);
        }

        assert!(!self.next.contains_key(&pos));

        self.blocked_positions.entry(pos.room_name()).or_default().insert(pos.xy());
        self.next.insert(pos, creep.clone());
        self.actions.insert(creep.clone(), action);
    }

    pub fn cancel_action_for(&mut self, creep: &SafeID<Creep>) {
        let action = self.actions.remove(creep).unwrap();
        assert_matches!(&action, MoveAction::Move { .. } | MoveAction::Pulled { .. });

        let pos = action.apply(creep.pos());
        self.next.remove(&pos);
        self.blocked_positions.get_mut(&pos.room_name()).unwrap().remove(&pos.xy());

        self.give_action_for(creep, MoveAction::Stay);
    }

    pub fn is_free_at(&self, pos: Position) -> bool {
        !self.next.contains_key(&pos)
    }

    pub fn execute(self) {
        todo!()
    }
}