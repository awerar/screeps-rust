use std::{collections::{HashMap, HashSet, VecDeque}, ops::Not};

use derive_deref::Deref;
use itertools::Itertools;
use screeps::{Creep, Direction, HasPosition, Position, RoomTerrain, Spawning, Terrain, game, look};
use serde::{Deserialize, Serialize};
use bimap::BiHashMap;

use crate::{memory::Memory, messages::SpawnMessage, safeid::{SafeID, deserialize_prune_hashmap_keys, deserialize_prune_hashset}};

pub struct MovementOpen;
#[derive(Serialize, Deserialize, Default)]
pub struct MovementClosed;

trait MovementTypeState { type Requests; }
impl MovementTypeState for MovementOpen { type Requests = HashMap<SafeID<Creep>, MoveRequest>; }
impl MovementTypeState for MovementClosed { type Requests = (); }

#[derive(Serialize, Deserialize, Default)]
#[expect(private_bounds)]
pub struct Movement<S : MovementTypeState = MovementOpen> {
    #[serde(deserialize_with = "deserialize_prune_hashset")]
    done_tugboats: HashSet<SafeID<Creep>>,

    #[serde(deserialize_with = "deserialize_prune_hashmap_keys")]
    paths: HashMap<SafeID<Creep>, (MoveTarget, VecDeque<Direction>)>,

    requests: S::Requests
}

impl Movement<MovementClosed> {
    pub fn open(self) -> Movement<MovementOpen> {
        Movement { 
            requests: HashMap::new(),
            done_tugboats: self.done_tugboats,
            paths: self.paths
        }
    }
}

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

enum MoveRequest {
    MoveTo(MoveTarget),
    TuggedMoveTo(MoveTarget),
    TugboatMove(Tugged)
}

impl MoveRequest {
    fn target(&self) -> Option<&MoveTarget> {
        match self {
            MoveRequest::MoveTo(target)
            | MoveRequest::TuggedMoveTo(target) => Some(target),
            _ => None
        }
    }
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

impl Movement<MovementOpen> {
    pub fn move_creep_to(&mut self, creep: &SafeID<Creep>, target: Position, range: u32) -> MoveToResult {
        let target = MoveTarget { target, range };
        let in_range = target.in_range(creep);

        self.requests.insert(creep.clone(), MoveRequest::MoveTo(target));
        if in_range { 
            MoveToResult::InRange 
        } else { 
            MoveToResult::OutOfRange 
        }
    }

    pub fn move_tugboat(&mut self, tugboat: &SafeID<Creep>, tugged: &SafeID<Creep>) -> MoveTugboatResult {
        if self.done_tugboats.contains(tugboat) {
            MoveTugboatResult::Done
        } else {
            self.requests.insert(tugboat.clone(), MoveRequest::TugboatMove(Tugged(tugged.clone())) );
            MoveTugboatResult::NotDone
        }
    }

    pub fn move_tugged_to(&mut self, creep: &SafeID<Creep>, target: Position, range: u32) -> MoveToResult {
        let target = MoveTarget { target, range };
        let in_range = target.in_range(creep);

        self.requests.insert(creep.clone(), MoveRequest::TuggedMoveTo(target));
        if in_range { 
            MoveToResult::InRange 
        } else { 
            MoveToResult::OutOfRange 
        }
    }

    pub fn close(self, mem: &mut Memory) -> Movement<MovementClosed> {
        let mut processor = MoveRequestProcessor::new(self.requests);
        let mut movement = Movement { 
            done_tugboats: self.done_tugboats, 
            paths: self.paths, 
            requests: () 
        };

        processor.collect_sessions();
        processor.handle_unpaired();
        processor.handle_distant_sessions();

        MovementSolver::new(processor.get_virtual_creeps()).solve();

        for tugged in &processor.unpaired_tugged {
            mem.messages.spawn.send(SpawnMessage::SpawnTugboatFor(tugged.0.clone()));
        }

        movement.done_tugboats = processor.unpaired_tugboats.into_iter().map(|tugboat| tugboat.0).collect();
        movement
    }
}

struct MoveRequestProcessor {
    requests: HashMap<SafeID<Creep>, MoveRequest>,
    sessions: BiHashMap<Tugboat, Tugged>,
    unpaired_tugboats: Vec<Tugboat>,
    unpaired_tugged: Vec<Tugged>,

    spawning: Vec<Spawning>
}

impl MoveRequestProcessor {
    fn new(requests: HashMap<SafeID<Creep>, MoveRequest>) -> MoveRequestProcessor {
        MoveRequestProcessor { 
            requests, 
            sessions: BiHashMap::new(),
            unpaired_tugboats: Vec::new(),
            unpaired_tugged: Vec::new(),
            spawning: Vec::new()
        }
    }

    fn get_tugged_target<'a>(&'a self, tugged: &Tugged) -> Option<&'a MoveTarget> {
        let MoveRequest::TuggedMoveTo(target) = self.requests.get(&tugged.0)? else { return None };
        Some(target)
    }

    fn collect_sessions(&mut self) {
        self.sessions = self.requests.iter()
            .filter_map(|(creep, request)| {
                let MoveRequest::TugboatMove(tugged) = request else { return None };
                self.get_tugged_target(tugged)?;

                Some((Tugboat(creep.clone()), tugged.clone()))
            }).collect();
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

    fn get_virtual_creeps(&mut self) -> HashMap<SafeID<Creep>, VirtualCreep> {
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
}

#[derive(Clone)]
enum VirtualCreep {
    Head { prev: SafeID<Creep> },
    Tail { next: SafeID<Creep>, target: Option<MoveTarget> },
    Single { target: Option<MoveTarget> }
}

impl VirtualCreep {
    fn is_single(&self) -> bool { matches!(self, Self::Single { .. }) }
    fn is_tail(&self) -> bool { matches!(self, Self::Tail { .. }) }
    fn is_head(&self) -> bool { matches!(self, Self::Head { .. }) }

    fn next(&self) -> Option<&SafeID<Creep>> {
        match self {
            Self::Tail { next, .. } => Some(next),
            _ => None
        }
    }

    fn prev(&self) -> Option<&SafeID<Creep>> {
        match self {
            Self::Head { prev } => Some(prev),
            _ => None
        }
    }

    fn target(&self) -> Option<&MoveTarget> {
        match self {
            Self::Tail { target, .. }
            | Self::Single { target } => target.as_ref(),
            _ => None
        }
    }
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

        let unsatisfied = self.creeps.iter().filter(|(creep, vcreep)| {
                vcreep.is_single()
                && vcreep.target().is_some_and(|target| target.in_range(*creep))
            }).map(|(creep, _)| creep.clone())
            .collect_vec();

        let tails = self.creeps.iter().filter(|(_, vcreep)| vcreep.is_tail())
            .map(|(creep, _)| creep.clone())
            .collect_vec();

        for creep in tails.into_iter().chain(unsatisfied.into_iter()) {
            self.poke(creep, &mut HashSet::new());
        }

        for creep in spawning {
            self.poke_spawning(&creep);
        }
    }

    fn poke_spawning(&mut self, creep: &Spawning) {

    }

    fn poke(&mut self, creep: SafeID<Creep>, visited: &mut HashSet<SafeID<Creep>>) -> bool {
        if self.next.get(&creep.pos()).is_some() { return false }
        if self.creeps.get(&creep).is_none_or(|vcreep| vcreep.is_head()) { return false }
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