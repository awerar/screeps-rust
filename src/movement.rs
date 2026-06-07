use std::collections::{HashMap, HashSet, VecDeque};

use derive_deref::Deref;
use itertools::Itertools;
use screeps::{Creep, Direction, HasPosition, Position, RoomTerrain, Spawning, StructureSpawn, Terrain, game, look};
use serde::{Deserialize, Serialize};
use bimap::BiHashMap;

use crate::{memory::Memory, safeid::{SafeID, deserialize_prune_hashmap_keys}};

/* 
===== Movement Solver Specification =====
Movement solver is responsible for tugboat destruction

There may ever only exist one tugboat for a given creep at one time

Creep chains try to destruct themselves as quickly as possible
Each segment in a chain has a target
Chains are evaluated from tail to the head
Chains are simplified so that just the head has a target

TODO: Force heads to move if they have any unsatisfied segments
*/

#[derive(Serialize, Deserialize, Clone)]
struct MoveTarget {
    pub target: Position, 
    pub range: u32
}

impl MoveTarget {
    pub fn in_range(&self, creep: &SafeID<Creep>) -> bool {
        creep.pos().get_range_to(self.target) <= self.range
    }
}

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

#[derive(Serialize, Deserialize, Default)]
pub struct MovementMemory {
    #[serde(deserialize_with = "deserialize_prune_hashmap_keys")]
    paths: HashMap<SafeID<Creep>, (MoveTarget, VecDeque<Direction>)>
}

pub struct Movement {
    singles: HashMap<SafeID<Creep>, MoveTarget>,
    sessions: BiHashMap<Tugboat, Tugged>,
    tugboats: HashMap<Tugboat, SafeID<StructureSpawn>>,
    tuggeds: HashMap<Tugged, MoveTarget>
}

impl Movement {
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
        let in_range = target.in_range(creep);

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
        let in_range = target.in_range(creep);

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
        self.handle_unpaired_tuggeds();

        let creeps = self.collect_creeps();
        let creeps = MovementSimplifier::new(creeps).simplify();

        MovementSolver::new(creeps, mem).solve();
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

    fn handle_unpaired_tuggeds(&mut self) {
        self.tuggeds.keys()
            .cloned()
            .collect::<HashSet<_>>()
            .difference(
                &self.sessions.right_values().cloned().collect()
            )
            .for_each(|tugged| {
                let target = self.tuggeds.remove(tugged).unwrap();
                self.singles.insert(tugged.0.clone(), target);
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
                        .map(|target| MoveCreep::Head { prev: None, target: target.clone() })
                        .unwrap_or(MoveCreep::Free)
                };

                (creep, move_creep)
            }).collect()
    }
}

#[derive(Clone)]
enum MoveCreep {
    Head { prev: Option<SafeID<Creep>>, target: MoveTarget },
    Follower { prev: Option<SafeID<Creep>>, next: SafeID<Creep>, target: MoveTarget },
    Free
}

impl MoveCreep {
    fn prev(&self) -> Option<&SafeID<Creep>> {
        match self {
            Self::Head { prev, .. }
            | Self::Follower { prev, .. } => prev.as_ref(),
            Self::Free => None
        }
    }

    fn next(&self) -> Option<&SafeID<Creep>> {
        match self {
            MoveCreep::Follower { next, ..} => Some(next),
            MoveCreep::Head { .. } | MoveCreep::Free => None,
        }
    }

    fn target(&self) -> Option<&MoveTarget> {
        match self {
            Self::Head { target, .. }
            | Self::Follower { target, .. } => Some(target),
            Self::Free => None
        }
    }
}

#[derive(Clone)]
enum SimpleMoveCreep {
    Head { prev: Option<SafeID<Creep>>, target: MoveTarget, must_move: bool },
    Follower { prev: Option<SafeID<Creep>>, next: SafeID<Creep> },
    Free,
    Stationary
}

struct MovementSimplifier {
    creeps: HashMap<SafeID<Creep>, MoveCreep>,
    result: HashMap<SafeID<Creep>, SimpleMoveCreep>
}

/* 
    All trains have a single target
    All trains are connected
    All creeps not marked as stationary can move
    No creeps are removed
*/
impl MovementSimplifier {
    pub fn new(creeps: HashMap<SafeID<Creep>, MoveCreep>) -> Self {
        Self {
            creeps,
            result: HashMap::new()
        }
    }

    pub fn simplify(mut self) -> HashMap<SafeID<Creep>, SimpleMoveCreep> {
        for (creep, mcreep) in self.creeps.iter().map(|(a, b)| (a.clone(), b.clone())).collect_vec() {
            match mcreep {
                MoveCreep::Free => {
                    if creep.fatigue() == 0 {
                        self.result.insert(creep.clone(), SimpleMoveCreep::Free);
                    } else {
                        self.result.insert(creep.clone(), SimpleMoveCreep::Stationary);
                    }
                }
                MoveCreep::Head { .. } => (),
                MoveCreep::Follower { prev, .. } => {
                    if prev.is_none() {
                        let new_tail = self.split_train(&creep);
                        self.simplify_train(&new_tail);
                    }
                },
            }
        }

        for (creep, mcreep) in self.result.iter().map(|(a, b)| (a.clone(), b.clone())).collect_vec() {
            let SimpleMoveCreep::Head { .. } = mcreep else { continue; };
            self.handle_train_fatigue(creep);
        }

        self.result
    }

    // Splits disconnected trains into smaller connected trains
    // Returns new tail (tail of frontmost train)
    fn split_train(&mut self, tail: &SafeID<Creep>) -> SafeID<Creep> {
        let mut train: VecDeque<SafeID<Creep>> = VecDeque::new();

        let mut segment = tail.clone();
        while let MoveCreep::Follower { prev, next, .. } = self.creeps.get(&segment).unwrap() {
            if !next.pos().is_near_to(segment.pos()) {
                self.result.insert(
                    segment.clone(), 
                    SimpleMoveCreep::Head { 
                        prev: prev.clone(), 
                        target: MoveTarget { target: next.pos(), range: 1 },
                        must_move: false
                    }
                );

                let mut follower_next = segment.clone();
                while let Some(follower) = train.pop_front() {
                    self.result.insert(
                        follower.clone(), 
                        SimpleMoveCreep::Follower { prev: train.front().cloned(), next: follower_next.clone() }
                    );
                    follower_next = follower;
                }
            } else {
                train.push_front(segment.clone());
            }

            segment = next.clone();
        }

        train.pop_back().unwrap_or_else(|| segment.clone())
    }

    fn simplify_train(&mut self, tail: &SafeID<Creep>) {
        let (target, must_move) = if let Some(prev) = self.creeps.get(tail).unwrap().prev() {
            // The train was split
            (MoveTarget { target: prev.pos(), range: 1 }, false)
        } else {
            let mut targets = VecDeque::new();
            let mut final_target = None;

            let mut segment = Some(tail.clone());
            while let Some(seg) = segment {
                let target = self.creeps.get(&seg).unwrap().target().unwrap();
                final_target = Some(target.clone());

                targets.push_front(target.clone());
                while targets.back().is_some_and(|target| target.in_range(&seg)) {
                    // We don't consider targets which will be achieved just by moving the snake
                    targets.pop_back();
                }

                segment = self.creeps.get(&seg).unwrap().next().cloned();
            }

            if let Some(target) = targets.pop_back() {
                (target, false)
            } else {
                (final_target.unwrap(), true)
            }
        };

        let mut segment = tail.clone();
        let mut prev_segment = None;
        while let MoveCreep::Follower { next, .. } = self.creeps.get(&segment).unwrap() {
            self.result.insert(
                segment.clone(), 
                SimpleMoveCreep::Follower { 
                    prev: prev_segment.clone(), 
                    next: segment.clone() 
                }
            );

            prev_segment = Some(segment);
            segment = next.clone();
        }

        self.result.insert(
            segment, 
            SimpleMoveCreep::Head { 
                prev: prev_segment, 
                target,
                must_move
            }
        );
    }

    fn handle_train_fatigue(&mut self, head: SafeID<Creep>) {
        todo!()
    }
}

struct MovementSolver<'m> {
    curr: HashMap<Position, SafeID<Creep>>,
    next: HashMap<Position, SafeID<Creep>>,

    creeps: HashMap<SafeID<Creep>, SimpleMoveCreep>,
    solution: HashMap<SafeID<Creep>, Option<Direction>>,

    mem: &'m mut Memory 
}

impl<'m> MovementSolver<'m> {
    fn new(creeps: HashMap<SafeID<Creep>, SimpleMoveCreep>, mem: &'m mut Memory) -> Self {
        MovementSolver { 
            curr: creeps.keys().cloned().map(|creep| (creep.pos(), creep)).collect(), 
            next: HashMap::new(), 
            creeps,
            solution: HashMap::new(),
            mem,
        }
    }

    fn solve(mut self) {
        let spawning = game::spawns().values()
            .filter_map(|spawn| spawn.spawning())
            .filter(|spawning| spawning.remaining_time() == 0)
            .collect_vec();

        /*let unsatisfied = self.creeps.iter().filter(|(creep, vcreep)| {
                vcreep.is_single()
                && vcreep.target().is_some_and(|target| target.in_range(*creep))
            }).map(|(creep, _)| creep.clone())
            .collect_vec();

        let tails = self.creeps.iter().filter(|(_, vcreep)| vcreep.is_tail())
            .map(|(creep, _)| creep.clone())
            .collect_vec();

        for creep in tails.into_iter().chain(unsatisfied.into_iter()) {
            self.poke(creep, &mut HashSet::new());
        }*/

        for creep in spawning {
            self.poke_spawning(&creep);
        }
    }

    fn poke_spawning(&mut self, creep: &Spawning) {

    }

    fn poke(&mut self, creep: SafeID<Creep>, visited: &mut HashSet<SafeID<Creep>>) -> bool {
        if self.next.get(&creep.pos()).is_some() { return false }
        //if self.creeps.get(&creep).is_none_or(|vcreep| vcreep.is_head()) { return false }
        if visited.contains(&creep) { return true }
        visited.insert(creep.clone());

        

        visited.remove(&creep);
        self.is_clear_at(creep.pos())
    }

    fn mcreep(&self, creep: SafeID<Creep>) -> &SimpleMoveCreep {
        self.creeps.get(&creep).unwrap()
    }

    fn is_clear_at(&self, pos: Position) -> bool {
        self.next.get(&pos).is_none() &&
        self.curr.get(&pos).is_none_or(|creep| self.solution.get(creep).is_some_and(|dir| dir.is_some()))
    }

    fn walkable_at(&self, pos: Position) -> bool {
        RoomTerrain::new(pos.room_name()).unwrap().get_xy(pos.xy()) != Terrain::Wall
        && pos.look_for(look::STRUCTURES).ok().is_none_or(|structures| {
            structures.into_iter().all(|structure| !structure.structure_type().is_obstacle())
        })
        && pos.look_for(look::CREEPS).ok().is_none_or(|creeps| {
            creeps.into_iter().all(|creep| creep.my())
        })
    }
}