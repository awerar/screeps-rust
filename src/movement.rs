use std::{collections::{HashMap, HashSet, VecDeque}, ops::Not};

use derive_deref::Deref;
use itertools::Itertools;
use screeps::{Creep, Direction, HasPosition, Position, RoomTerrain, Spawning, StructureSpawn, Terrain, game, look};
use serde::{Deserialize, Serialize};
use bimap::BiHashMap;

use crate::{memory::Memory, messages::SpawnMessage, safeid::{SafeID, deserialize_prune_hashmap_keys}};

/* 
===== Movement Solver Specification =====
Movement solver is responsible for tugboat destruction

There may ever only exist one tugboat for a given creep at one time

Creep chains try to destruct themselves as quickly as possible
Each segment in a chain has a target
Chains are evaluated from tail to the head
Chains are simplified so that just the head has a target
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
    }

    fn remove_invalid_sessions(&mut self) {
        self.sessions.right_values().cloned().collect::<HashSet<_>>()
            .difference(&self.tuggeds.keys().cloned().collect()).for_each(|tugged| {
                self.sessions.remove_by_right(tugged);
            });
    }

    fn handle_unpaired_tugboats(&mut self) {

    }

    fn handle_unpaired_tuggeds(&mut self) {

    }
}

/*struct ChainProcessor {
    requests: HashMap<SafeID<Creep>, MoveRequest>,
    sessions: BiHashMap<Tugboat, Tugged>
}

impl ChainProcessor {
    fn new(requests: HashMap<SafeID<Creep>, MoveRequest>) -> ChainProcessor {
        ChainProcessor { 
            requests, 
            sessions: BiHashMap::new()
        }
    }

    fn handle_unpaired(&mut self) {
        self.requests.iter()
            .filter(|(_, r)| matches!(r, MoveRequest::TugboatMove(_)))
            .map(|(creep, _)| Tugboat(creep.clone()))
            .collect::<HashSet<_>>()
            .difference(
                &self.sessions.left_values().cloned().collect()
            )
            .for_each(|tugboat| {
                self.requests.remove(tugboat);
                self.unpaired_tugboats.push(tugboat.clone());
            });

        self.requests.iter()
            .filter(|(_, r)| matches!(r, MoveRequest::TuggedMoveTo(_)))
            .map(|(creep, _)| Tugged(creep.clone()))
            .collect::<HashSet<_>>()
            .difference(
                &self.sessions.right_values().cloned().collect()
            )
            .for_each(|tugged| {
                let target = self.get_tugged_target(tugged).unwrap().clone();
                self.requests.insert(tugged.0.clone(), MoveRequest::MoveTo(target));
                self.unpaired_tugged.push(tugged.clone());
            });
    }

    fn handle_distant_sessions(&mut self) {
        self.sessions.iter()
            .filter(|(tugboat, tugged)| !tugboat.pos().is_near_to(tugged.pos()))
            .map(|(a, b)| (a.clone(), b.clone()))
            .collect_vec().into_iter()
            .for_each(|(tugboat, tugged)| {
                self.sessions.remove_by_left(&tugboat);

                let target = self.get_tugged_target(&tugged).unwrap().clone();
                self.requests.insert(tugged.0.clone(), MoveRequest::MoveTo(target));

                self.requests.insert(tugboat.0.clone(), MoveRequest::MoveTo(MoveTarget { target: tugged.pos(), range: 1 }));
            });
    }

    fn generate_virtual_creeps(&mut self) -> HashMap<SafeID<Creep>, VirtualCreep> {
        SafeID::creeps().flat_map(|creep| {
            let Some(request) = self.requests.get(&creep) else {
                return creep.spawning().not().then(||
                    (creep, VirtualCreep::Single { target: None }))
            };

            let virtual_creep = match request {
                MoveRequest::MoveTo(target) => 
                    VirtualCreep::Single { 
                        target: Some(target.clone()) 
                    },
                MoveRequest::TuggedMoveTo(target) => 
                    VirtualCreep::Tail { 
                        target: Some(target.clone()), 
                        next: self.sessions.get_by_right(&Tugged(creep.clone())).unwrap().0.clone()
                    },
                MoveRequest::TugboatMove(tugged) => 
                    VirtualCreep::Head { 
                        prev: tugged.0.clone()
                    },
            };

            Some((creep, virtual_creep))
        }).collect()
    }
}*/

enum MoveConstraint {
    PulledBy(SafeID<Creep>),
    GoTo(MoveTarget),
    Stay,
    Free
}

#[derive(Clone)]
enum VirtualCreep {
    Head { prev: SafeID<Creep> },
    Segment { prev: Option<SafeID<Creep>>, next: SafeID<Creep>, target: MoveTarget },
    Single { target: Option<MoveTarget> }
}

struct MovementSolver {
    curr: HashMap<Position, SafeID<Creep>>,
    next: HashMap<Position, SafeID<Creep>>,
    creeps: HashMap<SafeID<Creep>, VirtualCreep>,

    solution: HashMap<SafeID<Creep>, Option<Direction>>
}

impl MovementSolver {
    fn new(creeps: HashMap<SafeID<Creep>, VirtualCreep>) -> Self {
        MovementSolver { 
            curr: creeps.keys().cloned().map(|creep| (creep.pos(), creep)).collect(), 
            next: HashMap::new(), 
            creeps,
            solution: HashMap::new()
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

    fn vcreep(&self, creep: SafeID<Creep>) -> &VirtualCreep {
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