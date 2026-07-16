use std::collections::{HashMap, HashSet};

use bimap::BiHashMap;
use derive_deref::Deref;
use itertools::Itertools;
use nonempty::{NonEmpty, nonempty};
use screeps::{Creep, HasPosition, Position, RectStyle, RoomVisual, StructureSpawn, game};

use crate::{domain_traits::{CreepId, HasId, ResolvableId}, movement::{MoveTarget, MovementMemory, SpawningID, has_selected, simplifier::{RawMoveCreeps, RawTrain}, solver::MovementSolver}, spawn::TugboatRequests, statemachine::ShouldYield};

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deref)]
struct Tugboat(CreepId);

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deref)]
struct Tugged(CreepId);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MoveToResult {
    InRange, OutOfRange
}

impl MoveToResult {
    pub fn in_range(self) -> bool {
        matches!(self, Self::InRange)
    }
}

impl ShouldYield for MoveToResult {
    fn should_yield(&self) -> bool {
        !self.in_range()
    }
}

pub struct MovementRequests {
    singles: HashMap<CreepId, MoveTarget>,
    sessions: BiHashMap<Tugboat, Tugged>,
    tugboats: HashMap<Tugboat, StructureSpawn>,
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

    pub fn move_creep_to(&mut self, creep: &Creep, target: Position, range: u32) -> MoveToResult {
        let target = MoveTarget { target, range };
        let in_range = target.in_range(creep.pos());

        handle_target_visualization(creep, &target);

        self.singles.insert(creep.id(), target);
        if in_range { 
            MoveToResult::InRange 
        } else { 
            MoveToResult::OutOfRange 
        }
    }

    pub fn do_tugboat(&mut self, tugboat: &Creep, tugged: CreepId, spawn: &StructureSpawn) {
        self.tugboats.insert(Tugboat(tugboat.id()), spawn.clone());
        self.sessions.insert(Tugboat(tugboat.id()), Tugged(tugged));
    }

    pub fn move_tugged_to(&mut self, creep: &Creep, target: Position, range: u32) -> MoveToResult {
        let target = MoveTarget { target, range };
        let in_range = target.in_range(creep.pos());

        handle_target_visualization(creep, &target);

        if in_range { 
            MoveToResult::InRange 
        } else { 
            self.tuggeds.insert(Tugged(creep.id()), target);
            MoveToResult::OutOfRange 
        }
    }

    pub fn perform(mut self, mem: &mut MovementMemory) -> TugboatRequests {
        self.remove_invalid_sessions();
        self.handle_unpaired_tugboats();
        let tugboat_requests = self.handle_unpaired_tuggeds();

        MovementSolver::solve(self.collect_creeps().simplify(), mem);

        tugboat_requests
    }

    fn remove_invalid_sessions(&mut self) {
        // Tuggeds that didn't request to move
        self.sessions.right_values().cloned().collect::<HashSet<_>>()
            .difference(&self.tuggeds.keys().cloned().collect())
            .for_each(|tugged| {
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
                if tugboat.resolve().pos().is_near_to(spawn.pos()) {
                    spawn.recycle_creep(&tugboat.resolve()).ok();
                } else {
                    self.singles.insert(tugboat.0.clone(), MoveTarget { target: spawn.pos(), range: 1 });
                }
            });
    }

    fn handle_unpaired_tuggeds(&mut self) -> TugboatRequests {
        let mut tugboat_requests = TugboatRequests::new();

        self.tuggeds.keys()
            .cloned()
            .collect::<HashSet<_>>()
            .difference(
                &self.sessions.right_values().cloned().collect()
            )
            .for_each(|tugged| {
                let target = self.tuggeds.remove(tugged).unwrap();
                self.singles.insert(tugged.0.clone(), target);

                tugboat_requests.add_request_for(tugged.0.resolve());
            });

        tugboat_requests
    }

    fn collect_creeps(self) -> RawMoveCreeps {
        let free = game::creeps().values()
            .filter(|creep| {
                !self.sessions.contains_left(&Tugboat(creep.id())) &&
                !self.sessions.contains_right(&Tugged(creep.id())) &&
                !self.singles.contains_key(&creep.id()) &&
                !creep.spawning()
            }).collect_vec();

        let tug_trains = self.sessions.into_iter()
            .map(|(tugboat, tugged)| {
                let tugboat_target = MoveTarget { range: 1, target: self.tugboats.get(&tugboat).unwrap().pos() };
                let tugged_target = self.tuggeds.get(&tugged).unwrap().clone();

                RawTrain(nonempty![ 
                    (tugboat.resolve(), tugboat_target),
                    (tugged.resolve(), tugged_target)
                ])
            });

        let single_trains = self.singles.into_iter()
            .map(|(creep, target)| {
                RawTrain(NonEmpty::new((creep.resolve(), target)))
            });

        RawMoveCreeps {
            trains: tug_trains.chain(single_trains).collect(),
            free,
            spawning: game::spawns().values().filter_map(|spawn| SpawningID::new(&spawn)).collect(),
        }
    }
}

fn handle_target_visualization(creep: &Creep, target: &MoveTarget) {
    if has_selected(creep) {
        let visual = RoomVisual::new(Some(target.target.pos().room_name()));
        visual.rect(
            f32::from(target.target.x().u8() - target.range as u8), 
            f32::from(target.target.y().u8() - target.range as u8), 
            (2 * target.range) as f32, 
            (2 * target.range) as f32, 
            Some(RectStyle::default().fill("#2997ca"))
        );
    }
}