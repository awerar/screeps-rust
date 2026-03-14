use std::{cmp::min_by_key, collections::{HashMap, HashSet, VecDeque}, mem};

use derive_deref::Deref;
use itertools::Itertools;
use screeps::{Creep, Direction, HasPosition, Part, Position, game};
use serde::{Deserialize, Serialize};
use wasm_bindgen::convert::TryFromJsValue;

use crate::safeid::{SafeID, TryGetSafeID, deserialize_prune_hashmap_keys, deserialize_prune_hashset};

pub struct MovementOpen;
#[derive(Serialize, Deserialize, Default)]
pub struct MovementClosed;

trait MovementTypeState { type Requests; }
impl MovementTypeState for MovementOpen { type Requests = HashMap<SafeID<Creep>, MovementRequest>; }
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
            requests: game::spawns().values()
                .filter_map(|spawn| spawn.spawning())
                .filter(|spawning| spawning.remaining_time() == 0)
                .filter_map(|spawning| {
                    let creep = game::creeps().get(spawning.name().into())?.try_safe_id()?;
                    let request = MovementRequest::SpawnMove { pos: spawning.spawn().pos(), directions: spawning.directions().into_iter().map(|val| Direction::try_from_js_value(val).unwrap()).collect() };

                    Some((creep, request))
                }).collect(), 
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

enum MovementRequest {
    MoveTo(MoveTarget),
    TuggedMoveTo(MoveTarget),
    TugboatMove(Tugged),
    SpawnMove { pos: Position, directions: Vec<Direction> }
}

impl MovementRequest {
    fn target(&self) -> Option<&MoveTarget> {
        match self {
            MovementRequest::MoveTo(target)
            | MovementRequest::TuggedMoveTo(target) => Some(target),
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

        self.requests.insert(creep.clone(), MovementRequest::MoveTo(target));
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
            self.requests.insert(tugboat.clone(), MovementRequest::TugboatMove(Tugged(tugged.clone())) );
            MoveTugboatResult::NotDone
        }
    }

    pub fn move_tugged_to(&mut self, creep: &SafeID<Creep>, target: Position, range: u32) -> MoveToResult {
        let target = MoveTarget { target, range };
        let in_range = target.in_range(creep);

        self.requests.insert(creep.clone(), MovementRequest::TuggedMoveTo(target));
        if in_range { 
            MoveToResult::InRange 
        } else { 
            MoveToResult::OutOfRange 
        }
    }

    pub fn close(self) -> Movement<MovementClosed> {
        let requests = self.requests;
        let mut movement = Movement { 
            done_tugboats: self.done_tugboats, 
            paths: self.paths, 
            requests: () 
        };

        let tugboat2tugged: HashMap<_ ,_> = requests.iter()
            .filter_map(|(creep, request)| {
                let MovementRequest::TugboatMove(tugged) = request else { return None };
                Some((Tugboat(creep.clone()), tugged.clone()))
            }).collect();

        let tugged2tugboat: HashMap<_, _> = tugboat2tugged.iter()
            .map(|(tugboat, tugged)| (tugged.clone(), tugboat.clone()))
            .into_grouping_map()
            .reduce(|closest_tugboat, tugged, candidate_tugboat| {
                min_by_key(closest_tugboat, candidate_tugboat, |tugboat| {
                    tugged.pos().get_range_to(tugboat.pos())
                })
            }).into_iter()
            .filter_map(|(tugged, tugboat)| {
                let MovementRequest::TuggedMoveTo(target) = requests.get(&tugged)? else { return None };
                Some((tugged, (tugboat, target.clone())))
            }).collect();

        let used_tugboats: HashSet<_> = tugged2tugboat.values()
            .map(|(tugboat, _)| tugboat)
            .cloned()
            .collect();

        let all_tugboats = tugboat2tugged.keys()
            .cloned()
            .collect::<HashSet<_>>();

        let unused_tugboats: HashSet<_> = all_tugboats.difference(&used_tugboats)
            .cloned()
            .collect();

        let blocked = SafeID::creeps()
            .filter(|creep| {
                let has_move_parts = creep.body().into_iter().any(|bodypart| bodypart.part() == Part::Move);
                let is_fatigued = creep.fatigue() > 0;
                let is_used_tugboat = used_tugboats.contains(&Tugboat(creep.clone()));
                let will_be_tugged = tugged2tugboat.get(&Tugged(creep.clone()))
                    .is_some_and(|(tugboat, _)| tugboat.fatigue() == 0);

                is_fatigued || is_used_tugboat || !(has_move_parts || will_be_tugged)
            }).map(|creep| creep.pos())
            .collect::<HashSet<_>>();

        let shoveable: HashMap<_, _> = SafeID::creeps()
            .filter(|creep| {
                requests.get(creep).is_none_or(|request| {
                    let MovementRequest::MoveTo(target) = request else { return false };
                    target.in_range(creep)
                })
            }).filter(|creep| !blocked.contains(&creep.pos()))
            .map(|creep| {
                let target = requests.get(&creep).and_then(MovementRequest::target).cloned();
                (creep.pos(), (creep, target))
            }).collect();

        let moveable: HashMap<_, _> = SafeID::creeps()
            .filter_map(|creep| {
                let target = requests.get(&creep)?.target()?.clone();
                if target.in_range(&creep) { return None }

                Some((creep.pos(), (creep, target)))
            }).collect();

        let tugged: HashMap<_, _> = tugged2tugboat.into_iter()
            .map(|(tugged, (tugboat, target))| {
                (tugged.pos(), (tugged, tugboat, target))
            }).collect();

        let spawns = requests.into_values()
            .filter_map(|request| {
                let MovementRequest::SpawnMove { pos, directions } = request else { return None };
                Some((pos, directions))
            }).collect();

        let creeps: HashSet<_> = SafeID::creeps().map(|creep| creep.pos()).collect();

        MovementSolver { blocked, shoveable, moveable, creeps, tugged, spawns }
            .solve(&mut movement);

        movement.done_tugboats = unused_tugboats.into_iter().map(|tugboat| tugboat.0).collect();
        movement
    }
}

struct MovementSolver {
    blocked: HashSet<Position>,
    shoveable: HashMap<Position, (SafeID<Creep>, Option<MoveTarget>)>,
    moveable: HashMap<Position, (SafeID<Creep>, MoveTarget)>,
    creeps: HashSet<Position>,

    tugged: HashMap<Position, (Tugged, Tugboat, MoveTarget)>,
    spawns: HashMap<Position, Vec<Direction>>
}

impl MovementSolver {
    fn solve(mut self, movement: &mut Movement<MovementClosed>) {
        while let Some(pos) = self.tugged.keys().next() {
            self.try_clear(*pos, &mut HashSet::new());
        }

        while let Some(pos) = self.moveable.keys().next() {
            self.try_clear(*pos, &mut HashSet::new());
        }

        for (pos, directions) in mem::take(&mut self.spawns) {
            let positions = directions.into_iter()
                .map(|dir| pos + dir)
                .sorted_by_key(|pos| i32::from(self.creeps.contains(pos)));

            for pos in positions {
                if self.try_clear(pos, &mut HashSet::new()) {
                    break;
                }
            }

        }
    }

    fn try_clear(&mut self, pos: Position, cleared: &mut HashSet<Position>) -> bool {
        todo!()
    }
}