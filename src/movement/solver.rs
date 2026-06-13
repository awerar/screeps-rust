use std::{collections::{HashMap, VecDeque}, assert_matches};

use itertools::Itertools;
use js_sys::Array;
use log::warn;
use screeps::{CostMatrix, CostMatrixSet, Creep, Direction, HasPosition, Position, RoomName, RoomTerrain, Terrain, game, look, pathfinder::{self, MultiRoomCostResult, SearchOptions}};
use wasm_bindgen::JsValue;

use crate::{movement::{MoveTarget, MovementMemory, SpawningID, simplifier::{CreepConstraint, SimpleMoveCreeps}}, safeid::{SafeID, TryGetSafeID}, utils::adjacent_positions};

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
enum Entity {
    Spawning(SpawningID),
    Creep(SafeID<Creep>)
}

pub struct MovementSolver<'m> {
    creeps: SimpleMoveCreeps,
    mem: &'m mut MovementMemory,

    blocked_positions: HashMap<Position, Entity>,
    room_cost_matrix: HashMap<RoomName, CostMatrix>,

    spawning_actions: HashMap<SpawningID, Direction>,
    creep_actions: HashMap<SafeID<Creep>, CreepAction>
}

impl SimpleMoveCreeps {
    fn solve_order(&self) -> Vec<Entity> {
        self.creeps.keys().map(|creep| Entity::Creep(creep.clone()))
            .chain(self.spawning.iter().map(|spawning| Entity::Spawning(spawning.clone())))
            .sorted_by_cached_key(|entity| self.solve_priority(entity))
            .collect()
    }

    fn solve_priority(&self, entity: &Entity) -> usize {
        match entity {
            Entity::Spawning(_) => 3,
            Entity::Creep(creep) => {
                match self.creeps.get(creep).unwrap() {
                    CreepConstraint::Stay => 5,
                    CreepConstraint::Follow(_) => 4,
                    CreepConstraint::Move { target, must_move } => 
                        if *must_move || !target.in_range(creep.pos()) { 2 } else { 1 },
                    CreepConstraint::Free => 0,
                }
            },
        }
    }
}

impl<'m> MovementSolver<'m> {
    pub fn solve(creeps: SimpleMoveCreeps, mem: &'m mut MovementMemory) {
        let mut solver = Self {
            creeps,
            mem,
            blocked_positions: HashMap::new(),
            room_cost_matrix: HashMap::new(),
            spawning_actions: HashMap::new(),
            creep_actions: HashMap::new(),
        };

        for entity in solver.creeps.solve_order() {
            solver.solve_entity(&entity);
        }

        solver.execute();
    }

    fn give_creep_action(&mut self, creep: &SafeID<Creep>, action: CreepAction) {
        let pos = action.apply(creep.pos());

        let other = self.blocked_positions.get(&pos).cloned();
        if let Some(other) = &other  {
            assert_matches!(action, CreepAction::Stay, "Tried to schedule invalid move");
            self.cancel_action_for(other);
        }

        assert!(!self.blocked_positions.contains_key(&pos));

        self.room_cost_matrix.entry(pos.room_name()).or_insert_with(CostMatrix::new).set_xy(pos.xy(), 255);
        self.blocked_positions.insert(pos, Entity::Creep(creep.clone()));
        self.creep_actions.insert(creep.clone(), action);

        if let Some(other) = &other {
            self.solve_entity(other);
        }
    }

    fn give_spawning_action(&mut self, spawning: &SpawningID, direction: Direction) {
        let pos = spawning.pos() + direction;

        assert!(!self.blocked_positions.contains_key(&pos), "Tried to schedule invalid move");

        self.room_cost_matrix.entry(pos.room_name()).or_insert_with(CostMatrix::new).set_xy(pos.xy(), 255);
        self.blocked_positions.insert(pos, Entity::Spawning(spawning.clone()));
        self.spawning_actions.insert(spawning.clone(), direction);
    }

    fn cancel_action_for(&mut self, entity: &Entity) {
        let pos = match entity {
            Entity::Spawning(spawning) => {
                let direction = self.spawning_actions.remove(spawning).unwrap();
                spawning.pos() + direction
            },
            Entity::Creep(creep) => {
                let action = self.creep_actions.remove(creep).unwrap();
                assert_matches!(&action, CreepAction::Move { .. } | CreepAction::Pulled { .. });
                action.apply(creep.pos())
            },
        };

        self.blocked_positions.remove(&pos);
        self.room_cost_matrix.get_mut(&pos.room_name()).unwrap().set_xy(pos.xy(), 0);
    }

    fn solve_entity(&mut self, entity: &Entity) {
        match entity {
            Entity::Spawning(spawning) => 
                self.solve_spawning(spawning),
            Entity::Creep(creep) => 
                match self.creeps.creeps.get(creep).unwrap() {
                    CreepConstraint::Stay => 
                        self.give_creep_action(creep, CreepAction::Stay),
                    CreepConstraint::Follow(next) => 
                        if self.position_priority(next.pos()).is_some() {
                            self.give_creep_action(creep, CreepAction::Pulled { next: next.clone() });
                        } else {
                            self.give_creep_action(creep, CreepAction::Stay);
                        },
                    CreepConstraint::Move { target, must_move } => 
                        if target.in_range(creep.pos()) {
                            self.solve_local_move(creep, &target.clone(), *must_move);
                        } else {
                            self.solve_distant_move(creep, &target.clone());
                        },
                    CreepConstraint::Free => 
                        self.solve_free(creep),
                }
        }
    }

    fn solve_spawning(&mut self, spawning: &SpawningID) {
        let dirs = Direction::iter().copied();
        let dir = Self::best_by_priority(dirs, |dir| self.position_priority(spawning.pos() + *dir));
        if let Some(dir) = dir {
            self.give_spawning_action(spawning, dir);
        } else {
            warn!("Unable to find spawning direction");
        }
    }

    fn solve_distant_move(&mut self, creep: &SafeID<Creep>, target: &MoveTarget) {
        if self.try_move_by_path(creep, target) { return }

        let options = SearchOptions::default()
            .room_callback(|room|
                self.room_cost_matrix.get(&room).map_or(
                    MultiRoomCostResult::Default, 
                    |cm| MultiRoomCostResult::CostMatrix(cm.clone())
                )
            );

        let path = pathfinder::search(creep.pos(), target.target, target.range, Some(options)).path();
        let path = VecDeque::from_iter(path);

        self.mem.store_path(creep, target, path);
        if !self.try_move_by_path(creep, target) {
            self.give_creep_action(creep, CreepAction::Stay);
        }
    }

    fn try_move_by_path(&mut self, creep: &SafeID<Creep>, target: &MoveTarget) -> bool {
        let dir = self.mem.get_path_direction(creep, target);
        if let Some(dir) = dir && self.position_priority(creep.pos() + dir).is_some() {
            self.give_creep_action(creep, CreepAction::Move { dir });
            return true;
        }

        false
    }

    fn solve_local_move(&mut self, creep: &SafeID<Creep>, target: &MoveTarget, must_move: bool) {
        if !must_move && self.position_priority(creep.pos()).is_some() {
            self.give_creep_action(creep, CreepAction::Stay);
            return;
        }

        let next_pos = Self::best_by_priority(
            adjacent_positions(creep.pos()),
            |pos| {
                let prio = self.position_priority(*pos)?;
                let bias = if target.in_range(*pos) { 10 } else { 0 };
                Some(prio + bias)
            });

        let next_pos = next_pos.unwrap_or(creep.pos());
        let dir = creep.pos().get_direction_to(next_pos);

        if let Some(dir) = dir {
            self.give_creep_action(creep, CreepAction::Move { dir });
        } else {
            self.give_creep_action(creep, CreepAction::Stay);
        }
    }

    fn solve_free(&mut self, creep: &SafeID<Creep>) {
        self.solve_local_move(
            creep, 
            &MoveTarget { target: creep.pos(), range: 1 }, 
            false
        );
    }

    fn position_priority(&self, pos: Position) -> Option<usize> {
        if self.blocked_positions.contains_key(&pos) { return None }
        if RoomTerrain::new(pos.room_name()).unwrap().get_xy(pos.xy()) == Terrain::Wall { return None }

        let structures = pos.look_for(look::STRUCTURES).unwrap_or_default();
        if structures.iter().any(|structure| structure.structure_type().is_obstacle()) { return None }

        let creeps = pos.look_for(look::CREEPS).unwrap_or_default();
        let (my_creeps, enemy_creeps) = creeps.into_iter().partition::<Vec<_>, _>(Creep::my);
        if !enemy_creeps.is_empty() { return None }

        assert!(my_creeps.len() <= 1);
        let Some(other) = my_creeps.first().and_then(TryGetSafeID::try_safe_id) else { 
            return Some(2) 
        };

        match self.creeps.creeps.get(&other).unwrap() {
            CreepConstraint::Stay => None,
            CreepConstraint::Follow(_) 
            | CreepConstraint::Move { .. } => Some(1),
            CreepConstraint::Free => Some(0),
        }
    }

    fn best_by_priority<A>(iter: impl Iterator<Item = A>, prio: impl Fn(&A) -> Option<usize>) -> Option<A> {
        iter.filter_map(|x| prio(&x).map(|prio| (prio, x)))
            .max_by_key(|(prio, _)| *prio)
            .map(|(_, x)| x)
    }

    fn execute(self) {
        assert_eq!(self.creep_actions.len(), game::creeps().keys().try_len().unwrap());

        for (creep, action) in self.creep_actions {
            match action {
                CreepAction::Move { dir } => {
                    creep.move_direction(dir).unwrap();
                },
                CreepAction::Pulled { next } => {
                    creep.move_pulled_by(&next).unwrap();
                    next.pull(&creep).unwrap();
                },
                CreepAction::Stay => (),
            }
        }

        for (spawning, dir) in self.spawning_actions {
            spawning.set_directions(&Array::of1(&JsValue::from(dir as u8))).unwrap();
        }
    }
}

impl MovementMemory {
    fn get_path_direction(&mut self, creep: &SafeID<Creep>, target: &MoveTarget) -> Option<Direction> {
        let (old_target, path) = self.paths.get_mut(creep)?;
        if old_target == target {
            while path.front().is_some_and(|pos| *pos != creep.pos()) {
                path.pop_front();
            }

            let next_pos = *path.get(1)?;
            Some(creep.pos().get_direction_to(next_pos).unwrap())
        } else {
            self.paths.remove(creep);
            None
        }
    }

    fn store_path(&mut self, creep: &SafeID<Creep>, target: &MoveTarget, path: VecDeque<Position>) {
        self.paths.insert(creep.clone(), (target.clone(), path));
    }
}