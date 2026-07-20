use std::{collections::{HashMap, VecDeque}, mem};

use itertools::Itertools;
use nonempty::NonEmpty;
use screeps::{Creep, Direction, HasPosition, Part};

use crate::{domain_traits::{CreepId, HasId}, movement::{MoveTarget, SpawningID}, spawn::prototype::Body};

pub struct RawTrain(pub NonEmpty<(Creep, MoveTarget)>);
pub struct RawMoveCreeps {
    pub trains: Vec<RawTrain>,
    pub free: Vec<Creep>,
    pub spawning: Vec<SpawningID>
}

struct SimpleTrain {
    pub segments: NonEmpty<Creep>,
    pub target: MoveTarget,
    pub must_move: bool
}

pub enum CreepConstraint {
    Stay,
    Move { target: MoveTarget, must_move: bool },
    Follow(Creep),
    Free,
}

pub struct SimpleMoveCreeps {
    pub spawning: Vec<SpawningID>,
    pub creeps: HashMap<CreepId, CreepConstraint>
}

/* 
    All trains have a single target
    All trains are connected
    All creeps not marked as stationary can move
    No creeps are removed
    TODO: Handle train room boundaries
*/
impl RawMoveCreeps {
    pub fn simplify(self) -> SimpleMoveCreeps {
        let mut trains = Vec::new();
        for train in self.trains {
            let (new_raw_train, extra_trains) = train.split();
            trains.extend(extra_trains);
            trains.push(new_raw_train.simplify());
        }

        let (trains, stationary_trains): (Vec<_>, Vec<_>) = 
            trains.into_iter().partition(|train| {
                !train.pull_fatigue_backwards() && has_move_parts(train.segments.first())
            });

        let (free, stationary_free): (Vec<_>, Vec<_>) = 
            self.free.into_iter().partition(|creep| {
                creep.fatigue() == 0 && has_move_parts(creep)
            });

        let mut stationary = Vec::new();
        stationary.extend(stationary_trains.into_iter().flat_map(|train| train.segments));
        stationary.extend(stationary_free);

        let mut creeps = HashMap::new();
        creeps.extend(trains.into_iter().flat_map(SimpleTrain::collect_constraints));
        creeps.extend(free.into_iter().map(|creep| (creep.id(), CreepConstraint::Free)));
        creeps.extend(stationary.into_iter().map(|creep| (creep.id(), CreepConstraint::Stay)));

        let spawning = self.spawning.into_iter()
            .filter(|spawning| spawning.remaining_time() <= 1)
            .collect_vec();

        SimpleMoveCreeps { 
            spawning,
            creeps
        }
    }
}

fn has_move_parts(creep: &Creep) -> bool {
    Body::from(creep).part_count(Part::Move) > 0
}

impl RawTrain {
    // Split into disconnected trains and peel of end segments
    fn split(self) -> (RawTrain, Vec<SimpleTrain>) {
        let mut result = Vec::new();

        let mut iter = self.0.into_iter().rev();
        let mut train = VecDeque::from(vec![ iter.next().unwrap() ]);

        let mut pickup_creep = None;

        for (segment, target) in iter {
            let (head, head_target) = train.front().unwrap();

            if train.len() == 1 && head_target.in_range(head.pos()) {
                let mut old_train = mem::take(&mut train);
                let (head, head_target) = old_train.pop_front().unwrap();

                result.push(SimpleTrain { 
                    segments: NonEmpty::new(head),
                    target: head_target,
                    must_move: false
                });
            } else if !segment.pos().is_near_to(head.pos()) {
                pickup_creep = Some(head.clone());
                let old_train = mem::take(&mut train);

                result.push(SimpleTrain { 
                    segments: NonEmpty::collect(old_train).unwrap().map(|(a, _)| a), 
                    target: MoveTarget { target: segment.pos(), range: 1 }, 
                    must_move: false 
                });
            }

            train.push_front((segment, target));
        }

        let mut train = RawTrain(NonEmpty::collect(train).unwrap());
        if let Some(pickup_creep) = pickup_creep {
            train.0.last_mut().1 = MoveTarget { 
                target: pickup_creep.pos(), 
                range: 1
            }
        }

        (train, result)
    }

    // Figure out which target to move towards
    fn simplify(self) -> SimpleTrain {
        let mut targets = VecDeque::new();
        let mut must_move = false;

        for (segment, target) in self.0.iter().rev() {
            targets.push_front(target.clone());
            while targets.back().is_some_and(|prev_target| prev_target.in_range(segment.pos())) {
                targets.pop_back();
            }

            if !targets.is_empty() {
                must_move = true;
            }
        }

        let target = targets.pop_back().unwrap_or_else(|| self.0.first().1.clone());
        SimpleTrain { 
            segments: self.0.map(|(a, _)| a), 
            target, 
            must_move 
        }
    }
}

impl SimpleTrain {
    fn pull_fatigue_backwards(&self) -> bool {
        let Some((i, fatigued)) = self.segments.iter().find_position(|segment| segment.fatigue() > 0) else { return false };
        if i == 0 { return true; }

        let mut last_dir = None;
        for (ahead, behind) in self.segments.iter().take(i + 1).tuple_windows() {
            behind.pull(ahead).unwrap();
            ahead.move_pulled_by(behind).unwrap();

            last_dir = Some(ahead.pos().get_direction_to(behind.pos()).unwrap());
        }

        fatigued.move_direction(last_dir.unwrap_or(Direction::Right)).unwrap_err();

        true
    }

    fn collect_constraints(self) -> Vec<(CreepId, CreepConstraint)> {
        let mut constraints = Vec::new();
        constraints.push((
            self.segments.first().id(),
            CreepConstraint::Move { 
                target: self.target, 
                must_move: self.must_move 
            }
        ));

        constraints.extend(
            self.segments.iter()
                .tuple_windows()
                .map(|(ahead, behind)| {
                    (behind.id(), CreepConstraint::Follow(ahead.clone()))
                })
        );

        constraints
    }
}