use std::{cmp::Reverse, collections::{HashMap, HashSet, VecDeque}, assert_matches};

use derive_deref::Deref;
use itertools::Itertools;
use screeps::{CostMatrix, CostMatrixSet, Creep, Direction, FindPathOptions, HasPosition, Part, Path, Position, RoomName, RoomTerrain, RoomXY, Spawning, StructureSpawn, Terrain, find, game, look, pathfinder::{self, MultiRoomCostResult}};
use serde::{Deserialize, Serialize};
use bimap::BiHashMap;

use crate::{memory::Memory, messages::{Messages, SpawnMessage}, pathfinding, safeid::{SafeID, deserialize_prune_hashmap_keys}, spawn::Body};

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

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
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

    fn target(&mut self) -> Option<&MoveTarget> {
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

impl SimpleMoveCreep {
    fn prev(&self) -> Option<&SafeID<Creep>> {
        match self {
            Self::Head { prev, .. }
            | Self::Follower { prev, .. } => prev.as_ref(),
            Self::Free | Self::Stationary => None
        }
    }

    fn target(&self) -> Option<&MoveTarget> {
        match self {
            Self::Head { target, .. }  => Some(target),
            Self::Free | Self::Follower { .. } | Self::Stationary => None
        }
    }
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
            let detachable = target.in_range(&segment) && train.is_empty();

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
            prev.pull(segment);
            segment.move_pulled_by(&prev);

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

/*
We solve each creep one by one, and it must decide where to go just by looking at the partial solution
This assumes we can force any moveable creep to move elsewhere
If this assumption proves false we undo invalid moves and mark the creeps as stationary
- Head 

We need to decide which order to solve creeps in
*/

#[derive(Debug)]
enum MoveAction {
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

struct MovementSolver<'m> {
    creeps: HashMap<SafeID<Creep>, SimpleMoveCreep>,
    mem: &'m mut MovementMemory,
    solution: MovementSolution,
}

struct MovementSolution {
    next: HashMap<Position, SafeID<Creep>>,
    actions: HashMap<SafeID<Creep>, MoveAction>,
    room_blocks: HashMap<RoomName, HashSet<RoomXY>>
}

impl MovementSolution {
    fn new() -> Self {
        Self {
            actions: HashMap::new(),
            next: HashMap::new(),
            room_blocks: HashMap::new()
        }
    }

    fn give_action_for(&mut self, creep: &SafeID<Creep>, action: MoveAction) {
        let pos = action.apply(creep.pos());

        if let Some(other) = self.next.get(&pos).cloned() {
            if !matches!(action, MoveAction::Stay) {
                self.give_action_for(creep, MoveAction::Stay);
                return;
            }

            self.cancel_action_for(&other);
        }

        assert!(self.next.get(&pos).is_none());

        self.room_blocks.entry(pos.room_name()).or_default().insert(pos.xy());
        self.next.insert(pos, creep.clone());
        self.actions.insert(creep.clone(), action);
    }

    fn cancel_action_for(&mut self, creep: &SafeID<Creep>) {
        let action = self.actions.remove(creep).unwrap();
        assert_matches!(&action, MoveAction::Move { .. } | MoveAction::Pulled { .. });

        let pos = action.apply(creep.pos());
        self.next.remove(&pos);
        self.room_blocks.get_mut(&pos.room_name()).unwrap().remove(&pos.xy());

        self.give_action_for(creep, MoveAction::Stay);
    }

    fn is_free_at(&self, pos: &Position) -> bool {
        self.next.get(pos).is_none()
    }

    fn execute(self) {
        todo!()
    }
}

impl MovementMemory {
    fn pop_path_direction_for(&mut self, creep: &SafeID<Creep>, target: &MoveTarget) -> Option<Direction> {
        let (old_target, path) = self.paths.get_mut(creep)?;
        if old_target == target {
            Some(path.pop_back().unwrap())
        } else {
            self.paths.remove(creep);
            None
        }
    }

    fn store_path(&mut self, creep: &SafeID<Creep>, target: &MoveTarget, path: VecDeque<Direction>) {
        self.paths.insert(creep.clone(), (target.clone(), path));
    }
}

impl<'m> MovementSolver<'m> {
    fn new(creeps: HashMap<SafeID<Creep>, SimpleMoveCreep>, mem: &'m mut MovementMemory) -> Self {
        MovementSolver { 
            creeps,
            mem,
            solution: MovementSolution::new()
        }
    }

    fn solve(mut self) {
        self.solve_stationary();
        self.solve_pathing_heads();
        self.solve_spawning();
        self.solve_non_pathing_heads();
        self.solve_free();
        self.solution.execute();
    }

    fn solve_stationary(&mut self) {
        for (creep, mcreep) in &self.creeps {
            let SimpleMoveCreep::Stationary = mcreep else { continue; };
            self.solution.give_action_for(creep, MoveAction::Stay);
        }
    }

    fn solve_pathing_heads(&mut self) {
        self.creeps.iter()
            .filter_map(|(creep, mcreep)| {
                let SimpleMoveCreep::Head { target, .. } = mcreep else { return None };
                Some((creep, target))
            }).filter(|(creep, target)| !target.in_range(creep))
            .sorted_by_cached_key(|(creep, _)| Reverse(self.train_length(creep)))
            .map(|(a, b)| (a.clone(), b.clone()))
            .collect_vec().into_iter()
            .for_each(|(head, target)| self.solve_pathing_head(&head, &target));
    }

    fn solve_pathing_head(&mut self, head: &SafeID<Creep>, target: &MoveTarget) {
        if self.try_move_train_by_path(head, target) { return }

        let options = FindPathOptions::<fn(_, CostMatrix) -> MultiRoomCostResult, MultiRoomCostResult>::default()
            .range(target.range)
            .ignore_creeps(true)
            .cost_callback(|room, mut cost_matrix| {
                for xy in self.solution.room_blocks.entry(room).or_default() {
                    cost_matrix.set_xy(*xy, 255);
                }

                MultiRoomCostResult::CostMatrix(cost_matrix)
            });

        let path = head.pos().find_path_to(&target.target, Some(options));
        let Path::Vectorized(path) = path else { unreachable!() };
        let path = path.into_iter().map(|step| step.direction).collect();

        self.mem.store_path(head, target, path);
        if !self.try_move_train_by_path(head, target) {
            self.make_train_stay(head);
        }
    }

    fn train_length(&self, creep: &SafeID<Creep>) -> usize {
        self.creeps.get(&creep).unwrap().prev().map_or(1, |prev| self.train_length(prev) + 1)
    }

    fn try_move_train_by_path(&mut self, head: &SafeID<Creep>, target: &MoveTarget) -> bool {
        let path_dir = self.mem.pop_path_direction_for(head, target);
        if let Some(path_dir) = path_dir {
            if self.solution.is_free_at(&(head.pos() + path_dir)) {
                self.make_train_move(head, path_dir);
                return true;
            }
        }

        false
    }

    fn make_train_stay(&mut self, creep: &SafeID<Creep>) {
        self.solution.give_action_for(creep, MoveAction::Stay);
        if let Some(prev) = self.creeps.get(creep).unwrap().prev() {
            self.make_train_stay(&prev.clone());
        }
    }

    fn make_train_move(&mut self, head: &SafeID<Creep>, dir: Direction) {
        self.solution.give_action_for(head, MoveAction::Move { dir });

        let mut next = head.clone();
        while let Some(curr) = self.creeps.get(&next).unwrap().prev() {
            self.solution.give_action_for(curr, MoveAction::Pulled { next });
            next = curr.clone();
        }
    }

    fn solve_non_pathing_heads(&mut self) {

    }

    fn solve_spawning(&mut self) {
        game::spawns().values()
            .filter_map(|spawn| spawn.spawning())
            .filter(|spawning| spawning.remaining_time() == 0)
            .for_each(|spawning| {
                self.poke_spawning(&spawning);
            });
    }

    fn poke_spawning(&mut self, creep: &Spawning) {

    }

    fn solve_free(&mut self) {

    }

    fn mcreep(&self, creep: SafeID<Creep>) -> &SimpleMoveCreep {
        self.creeps.get(&creep).unwrap()
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