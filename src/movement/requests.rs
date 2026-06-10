use std::collections::{HashMap, HashSet};

use bimap::BiHashMap;
use derive_deref::Deref;
use screeps::{Creep, HasPosition, Position, StructureSpawn};

use crate::{memory::Memory, messages::{Messages, SpawnMessage}, movement::{MoveTarget, simplifier::{MoveCreep, MovementSimplifier}, solver::MovementSolver}, safeid::SafeID};

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deref)]
struct Tugboat(SafeID<Creep>);

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deref)]
struct Tugged(SafeID<Creep>);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MoveToResult {
    InRange, OutOfRange
}

impl MoveToResult {
    pub fn in_range(self) -> bool {
        matches!(self, Self::InRange)
    }
}

pub struct MovementRequests {
    singles: HashMap<SafeID<Creep>, MoveTarget>,
    sessions: BiHashMap<Tugboat, Tugged>,
    tugboats: HashMap<Tugboat, SafeID<StructureSpawn>>,
    tuggeds: HashMap<Tugged, MoveTarget>
}

impl MovementRequests {
    pub fn new() -> Self {
        Self { 
            singles: HashMap::new(),
            tugboats: HashMap::new(),
            tuggeds: HashMap::new(),
            sessions: BiHashMap::new()
        }
    }

    pub fn move_creep_to(&mut self, creep: &SafeID<Creep>, target: Position, range: u32) -> MoveToResult {
        let target = MoveTarget { target, range };
        let in_range = target.in_range(creep.pos());

        self.singles.insert(creep.clone(), target);
        if in_range { 
            MoveToResult::InRange 
        } else { 
            MoveToResult::OutOfRange 
        }
    }

    pub fn do_tugboat(&mut self, tugboat: &SafeID<Creep>, tugged: &SafeID<Creep>, spawn: &SafeID<StructureSpawn>) {
        self.tugboats.insert(Tugboat(tugboat.clone()), spawn.clone());
        self.sessions.insert(Tugboat(tugboat.clone()), Tugged(tugged.clone()));
    }

    pub fn move_tugged_to(&mut self, creep: &SafeID<Creep>, target: Position, range: u32) -> MoveToResult {
        let target = MoveTarget { target, range };
        let in_range = target.in_range(creep.pos());

        self.tuggeds.insert(Tugged(creep.clone()), target);
        if in_range { 
            MoveToResult::InRange 
        } else { 
            MoveToResult::OutOfRange 
        }
    }

    pub fn perform(mut self, mem: &mut Memory) {
        self.remove_invalid_sessions();
        self.handle_unpaired_tugboats();
        self.handle_unpaired_tuggeds(&mut mem.messages);

        let creeps = self.collect_creeps();
        let creeps = MovementSimplifier::new(creeps).simplify();

        MovementSolver::new(creeps, &mut mem.movement).solve();
    }

    fn remove_invalid_sessions(&mut self) {
        self.sessions.right_values().cloned().collect::<HashSet<_>>()
            .difference(&self.tuggeds.keys().cloned().collect()).for_each(|tugged| {
                self.sessions.remove_by_right(tugged);
            });
    }

    fn handle_unpaired_tugboats(&mut self) {
        self.tugboats.keys()
            .cloned()
            .collect::<HashSet<_>>()
            .difference(
                &self.sessions.left_values().cloned().collect()
            )
            .for_each(|tugboat| {
                let spawn = self.tugboats.remove(tugboat).unwrap();
                if tugboat.pos().is_near_to(spawn.pos()) {
                    spawn.recycle_creep(tugboat).ok();
                } else {
                    self.singles.insert(tugboat.0.clone(), MoveTarget { target: spawn.pos(), range: 1 });
                }
            });
    }

    fn handle_unpaired_tuggeds(&mut self, messages: &mut Messages) {
        self.tuggeds.keys()
            .cloned()
            .collect::<HashSet<_>>()
            .difference(
                &self.sessions.right_values().cloned().collect()
            )
            .for_each(|tugged| {
                let target = self.tuggeds.remove(tugged).unwrap();
                self.singles.insert(tugged.0.clone(), target);

                messages.spawn.send(SpawnMessage::SpawnTugboatFor(tugged.0.clone()));
            });
    }

    fn collect_creeps(self) -> HashMap<SafeID<Creep>, MoveCreep> {
        SafeID::creeps().map(|creep| {
                let move_creep = if let Some(tugged) = self.sessions.get_by_left(&Tugboat(creep.clone())) {
                    MoveCreep::Head { 
                        prev: Some(tugged.0.clone()), 
                        target: MoveTarget { 
                            target: self.tugboats.get(&Tugboat(creep.clone())).unwrap().pos(), 
                            range: 1
                        } 
                    }
                } else if let Some(tugboat) = self.sessions.get_by_right(&Tugged(creep.clone())) {
                    MoveCreep::Follower { 
                        prev: None, 
                        next: tugboat.0.clone(), 
                        target: self.tuggeds.get(&Tugged(creep.clone())).unwrap().clone()
                    }
                } else {
                    self.singles.get(&creep)
                        .map_or(
                            MoveCreep::Free, 
                            |target| MoveCreep::Head { prev: None, target: target.clone() }
                        )
                };

                (creep, move_creep)
            }).collect()
    }
}