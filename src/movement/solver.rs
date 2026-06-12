use std::{cmp::Reverse, collections::{HashMap, HashSet, VecDeque}, assert_matches};

use itertools::Itertools;
use log::warn;
use screeps::{CostMatrix, CostMatrixSet, Creep, Direction, FindPathOptions, HasPosition, Path, Position, RoomName, RoomTerrain, RoomXY, Spawning, Terrain, game, look, pathfinder::MultiRoomCostResult};
use serde::{Deserialize, Serialize};

use crate::{movement::{MoveTarget, SpawningID, simplifier::SimpleMoveCreeps}, safeid::{SafeID, deserialize_prune_hashmap_keys}, utils::adjacent_positions};

#[derive(Debug)]
pub enum CreepAction {
    Move { dir: Direction },
    Pulled { next: SafeID<Creep> },
    Stay
}

impl CreepAction {
    fn apply(&self, pos: Position) -> Position {
        match self {
            CreepAction::Move { dir } => pos + *dir,
            CreepAction::Pulled { next } => next.pos(),
            CreepAction::Stay => pos,
        }
    }
}

#[derive(Clone)]
pub enum Entity {
    Spawning(SpawningID),
    Creep(SafeID<Creep>)
}

pub struct MovementSolution {
    blocked_positions: HashMap<Position, Entity>,
    room_blocked_xy: HashMap<RoomName, HashSet<RoomXY>>,

    spawning_actions: HashMap<SpawningID, Direction>,
    creep_actions: HashMap<SafeID<Creep>, CreepAction>
}

impl MovementSolution {
    pub fn new() -> Self {
        Self {
            blocked_positions: HashMap::new(),
            room_blocked_xy: HashMap::new(),
            spawning_actions: HashMap::new(),
            creep_actions: HashMap::new()
        }
    }

    pub fn give_creep_action(&mut self, creep: &SafeID<Creep>, action: CreepAction) {
        let pos = action.apply(creep.pos());

        if let Some(other) = self.blocked_positions.get(&pos).cloned() {
            if !matches!(action, CreepAction::Stay) {
                self.give_creep_action(creep, CreepAction::Stay);
                return;
            }

            self.cancel_action_for(&other);
        }

        assert!(!self.blocked_positions.contains_key(&pos));

        self.room_blocked_xy.entry(pos.room_name()).or_default().insert(pos.xy());
        self.blocked_positions.insert(pos, Entity::Creep(creep.clone()));
        self.creep_actions.insert(creep.clone(), action);
    }

    pub fn cancel_action_for(&mut self, entity: &Entity) {
        match entity {
            Entity::Spawning(spawning) => {
                self.spawning_actions.remove(spawning);
            },
            Entity::Creep(creep) => {
                let action = self.creep_actions.remove(creep).unwrap();
                assert_matches!(&action, CreepAction::Move { .. } | CreepAction::Pulled { .. });

                let pos = action.apply(creep.pos());
                self.next.remove(&pos);
                self.room_blocked_xy.get_mut(&pos.room_name()).unwrap().remove(&pos.xy());

                self.give_creep_action(creep, CreepAction::Stay);
            },
        }
    }

    pub fn is_free_at(&self, pos: Position) -> bool {
        !self.next.contains_key(&pos)
    }

    pub fn execute(self) {
        todo!()
    }
}

#[derive(Serialize, Deserialize, Default)]
pub struct MovementMemory {
    #[serde(deserialize_with = "deserialize_prune_hashmap_keys")]
    paths: HashMap<SafeID<Creep>, (MoveTarget, VecDeque<Direction>)>
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

pub fn solve(creeps: SimpleMoveCreeps, mem: &mut MovementMemory) -> MovementSolution {
    let solution = MovementSolution::new();

    self.solve_stationary();
    self.solve_pathing_heads();
    self.solve_spawnings();
    self.solve_non_pathing_heads();
    self.solve_free();
    
    solution
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
        }).filter(|(creep, target)| !target.in_range(creep.pos()))
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
            for xy in self.solution.blocked_positions.entry(room).or_default().iter() {
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
    self.creeps.get(creep).unwrap().prev().map_or(1, |prev| self.train_length(prev) + 1)
}

fn try_move_train_by_path(&mut self, head: &SafeID<Creep>, target: &MoveTarget) -> bool {
    let path_dir = self.mem.pop_path_direction_for(head, target);
    if let Some(path_dir) = path_dir
        && self.solution.is_free_at(head.pos() + path_dir) {
            self.make_train_move(head, path_dir);
            return true;
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
    self.creeps.iter()
        .filter_map(|(creep, mcreep)| {
            let SimpleMoveCreep::Head { target, must_move, .. } = mcreep else { return None };
            Some((creep, target, must_move))
        }).filter(|(creep, target, _)| target.in_range(creep.pos()))
        .sorted_by_cached_key(|(creep, _, _)| Reverse(self.train_length(creep)))
        .map(|(a, b, c)| (a.clone(), b.clone(), c.clone()))
        .collect_vec().into_iter()
        .for_each(|(head, target, must_move)| self.solve_non_pathing_head(&head, &target, must_move));
}

fn solve_non_pathing_head(&mut self, head: &SafeID<Creep>, target: &MoveTarget, must_move: bool) {
    if !must_move && self.solution.is_free_at(head.pos()) {
        self.make_train_stay(head);
        return;
    }

    let mut possible = adjacent_positions(head.pos()).collect_vec();
    if !must_move { possible.push(head.pos()); }

    let next_pos = possible.into_iter()
        .filter_map(|pos| {
            let prio = self.position_priority(pos)?;
            let bias = if target.in_range(pos) { 10 } else { 0 };
            Some((prio + bias, pos))
        }).max_by_key(|(prio, _)| Reverse(*prio))
        .map(|(_, pos)| pos);

    let next_pos = next_pos.unwrap_or(head.pos());
    let dir = head.pos().get_direction_to(next_pos);

    if let Some(dir) = dir {
        self.make_train_move(head, dir);
    } else {
        self.make_train_stay(head);
    }
}

fn position_priority(&self, pos: Position) -> Option<usize> {
    if !Self::walkable_at(pos) { return None }
    if !self.solution.is_free_at(pos) { return None }
    let Some(other) = self.curr.get(&pos) else { return Some(3) };

    match self.creeps.get(other).unwrap() {
        SimpleMoveCreep::Head { prev, .. } 
        | SimpleMoveCreep::Follower { prev, .. } if prev.is_some() => None,
        SimpleMoveCreep::Head { must_move, .. } => if *must_move { Some(2) } else { Some(0) },
        SimpleMoveCreep::Follower { .. } => Some(2),
        SimpleMoveCreep::Free => Some(1),
        SimpleMoveCreep::Stationary => None,
    }
}

fn solve_spawnings(&mut self) {
    game::spawns().values()
        .filter_map(|spawn| spawn.spawning())
        .filter(|spawning| spawning.remaining_time() == 0)
        .for_each(|spawning| {
            self.solve_spawning(&spawning);
        });
}

fn solve_spawning(&mut self, spawning: &Spawning) {
    
}

fn solve_free(&mut self) {

}

fn walkable_at(pos: Position) -> bool {
    RoomTerrain::new(pos.room_name()).unwrap().get_xy(pos.xy()) != Terrain::Wall
    && pos.look_for(look::STRUCTURES).ok().is_none_or(|structures| {
        structures.into_iter().all(|structure| !structure.structure_type().is_obstacle())
    })
    && pos.look_for(look::CREEPS).ok().is_none_or(|creeps| {
        creeps.into_iter().all(|creep| creep.my())
    })
}