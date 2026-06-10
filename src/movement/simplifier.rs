use std::collections::{HashMap, VecDeque};

use itertools::Itertools;
use screeps::{Creep, HasPosition, Part};

use crate::{movement::MoveTarget, safeid::SafeID, spawn::Body};

#[derive(Clone)]
pub enum MoveCreep {
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
pub enum SimpleMoveCreep {
    Head { prev: Option<SafeID<Creep>>, target: MoveTarget, must_move: bool },
    Follower { prev: Option<SafeID<Creep>>, next: SafeID<Creep> },
    Free,
    Stationary
}

impl SimpleMoveCreep {
    pub fn prev(&self) -> Option<&SafeID<Creep>> {
        match self {
            Self::Head { prev, .. }
            | Self::Follower { prev, .. } => prev.as_ref(),
            Self::Free | Self::Stationary => None
        }
    }
}

pub struct MovementSimplifier {
    creeps: HashMap<SafeID<Creep>, MoveCreep>,
    result: HashMap<SafeID<Creep>, SimpleMoveCreep>
}

/* 
    All trains have a single target
    All trains are connected
    All creeps not marked as stationary can move
    No creeps are removed
    TODO: Handle train room boundaries
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
            self.handle_train_moveability(&creep);
        }

        self.result
    }

    // Splits disconnected trains into smaller connected trains
    // Also peels away tail segments which satisfy their target
    // Returns new tail (tail of frontmost train)
    fn split_train(&mut self, tail: &SafeID<Creep>) -> SafeID<Creep> {
        let mut train: VecDeque<SafeID<Creep>> = VecDeque::new();

        let mut segment = tail.clone();
        while let MoveCreep::Follower { prev, next, target } = self.creeps.get(&segment).unwrap() {
            let disconnected = !next.pos().is_near_to(segment.pos());
            let detachable = target.in_range(segment.pos()) && train.is_empty();

            if disconnected || detachable {
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
        let (target, must_move) = {
            if let Some(prev) = self.creeps.get(tail).unwrap().prev() {
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
                    while targets.back().is_some_and(|target| target.in_range(seg.pos())) {
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

    fn handle_train_moveability(&mut self, head: &SafeID<Creep>) {
        if Body::from(&**head).num(Part::Move) == 0 || self.pull_backwards_rec(head) {
            self.make_train_stationary(head);
        }
    }

    // Pulls the train backwards from any fatigued creep
    // This makes it recieve negative fatigue from the head
    fn pull_backwards_rec(&mut self, segment: &SafeID<Creep>) -> bool {
        if segment.fatigue() > 0 { return true; }

        let Some(prev) = self.result.get(segment).unwrap().prev().cloned() else { return false };
        if self.pull_backwards_rec(&prev) {
            prev.pull(segment).ok();
            segment.move_pulled_by(&prev).ok();

            true
        } else {
            false
        }
    }

    fn make_train_stationary(&mut self, head: &SafeID<Creep>) {
        let mut segment = Some(head.clone());
        while let Some(seg) = &segment {
            segment = self.result.insert(seg.clone(), SimpleMoveCreep::Stationary).unwrap().prev().cloned();
        }
    }
}