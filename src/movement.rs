use std::{cmp::min_by_key, collections::{HashMap, HashSet, VecDeque}};

use derive_deref::Deref;
use itertools::Itertools;
use screeps::{Creep, Direction, HasPosition, Part, Position};
use serde::{Deserialize, Serialize};

use crate::safeid::{SafeID, deserialize_prune_hashset, deserialize_prune_hashmap_keys};

#[derive(Serialize, Deserialize, Default)]
pub struct Movement {
    #[serde(deserialize_with = "deserialize_prune_hashset")]
    done_tugboats: HashSet<SafeID<Creep>>,

    #[serde(deserialize_with = "deserialize_prune_hashmap_keys")]
    paths: HashMap<SafeID<Creep>, (MoveTarget, VecDeque<Direction>)>,

    #[serde(default, skip)]
    requests: HashMap<SafeID<Creep>, MovementRequest>
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

impl Movement {
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

    /*
        Blocked: positions that will contain a creep at the end of the tick
            Initialized by:
                Creeps with no MOVE part and (!being tugged or tugboat fatigued)
                Fatigued creeps
                Tugboats

        Shovable creeps: positions with creeps that can could be moved out of the way
            Creeps with no move request or that are their target (tugged and normal)
            Shoving can fail if no spot within target is found
            After an attempted shove blocked is updated

        Moving creeps: positions with creeps that are expected to unblock this tick
            Creeps that aren't at their target (tugged and normal)

    
     */

    pub fn move_all(&mut self) {
        let tugboat2tugged: HashMap<_ ,_> = self.requests.iter()
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
                let MovementRequest::TuggedMoveTo(target) = self.requests.get(&tugged)? else { return None };
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
                self.requests.get(creep).is_none_or(|request| {
                    let MovementRequest::MoveTo(target) = request else { return false };
                    target.in_range(creep)
                })
            }).filter(|creep| !blocked.contains(&creep.pos()))
            .map(|creep| {
                let target = self.requests.get(&creep).and_then(MovementRequest::target).cloned();
                (creep.pos(), (creep, target))
            }).collect();

        let moveable: HashMap<_, _> = SafeID::creeps()
            .filter_map(|creep| {
                let target = self.requests.get(&creep)?.target()?.clone();
                if target.in_range(&creep) { return None };

                Some((creep.pos(), (creep, target)))
            }).collect();

        MovementSolver {
            blocked,
            shoveable,
            moveable,
            tugged2tugboat,
        }.solve();

        self.done_tugboats = unused_tugboats.into_iter().map(|tugboat| tugboat.0).collect();
    }
}

struct MovementSolver {
    blocked: HashSet<Position>,
    shoveable: HashMap<Position, (SafeID<Creep>, Option<MoveTarget>)>,
    moveable: HashMap<Position, (SafeID<Creep>, MoveTarget)>,

    tugged2tugboat: HashMap<Tugged, (Tugboat, MoveTarget)>
}

impl MovementSolver {
    fn solve(self) {

    }
}