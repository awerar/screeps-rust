use std::{assert_matches, cmp::Reverse, collections::{HashMap, VecDeque}};

use itertools::Itertools;
use js_sys::Array;
use log::warn;
use screeps::{CostMatrix, CostMatrixSet, Creep, Direction, HasPosition, Position, RoomName, RoomTerrain, StructureType, Terrain, find, game, look, pathfinder::{self, MultiRoomCostResult, SearchOptions}};
use wasm_bindgen::JsValue;

use crate::{movement::{CachedPath, MoveTarget, MovementMemory, SpawningID, simplifier::{CreepConstraint, SimpleMoveCreeps}}, ids::{CheckedID, TryGetCheckedID}, utils::adjacent_positions};

#[derive(Debug)]
pub enum CreepAction {
    Move { dir: Direction },
    Pulled { next: CheckedID<Creep> },
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
    Creep(CheckedID<Creep>)
}

pub struct MovementSolver<'m> {
    creeps: SimpleMoveCreeps,
    mem: &'m mut MovementMemory,

    blocked_positions: HashMap<Position, Entity>,
    costmatrix_cache: HashMap<RoomName, CostMatrix>,

    spawning_actions: HashMap<SpawningID, Direction>,
    creep_actions: HashMap<CheckedID<Creep>, CreepAction>
}

impl SimpleMoveCreeps {
    fn solve_order(&self) -> Vec<Entity> {
        self.creeps.keys().map(|creep| Entity::Creep(creep.clone()))
            .chain(self.spawning.iter().map(|spawning| Entity::Spawning(spawning.clone())))
            .sorted_by_cached_key(|entity| Reverse(self.solve_priority(entity)))
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
            spawning_actions: HashMap::new(),
            creep_actions: HashMap::new(),
            costmatrix_cache: HashMap::new()
        };

        for entity in solver.creeps.solve_order() {
            solver.solve_entity(&entity);
        }

        solver.execute();
    }

    fn give_creep_action(&mut self, creep: &CheckedID<Creep>, action: CreepAction) {
        let pos = action.apply(creep.pos());

        let other = self.blocked_positions.get(&pos).cloned();
        if let Some(other) = &other  {
            assert_matches!(action, CreepAction::Stay, "Tried to schedule invalid move");
            self.cancel_action_for(other);
        }

        assert!(!self.blocked_positions.contains_key(&pos));

        self.blocked_positions.insert(pos, Entity::Creep(creep.clone()));
        self.creep_actions.insert(creep.clone(), action);

        if let Some(other) = &other {
            self.solve_entity(other);
        }
    }

    fn give_spawning_action(&mut self, spawning: &SpawningID, direction: Direction) {
        let pos = spawning.pos() + direction;

        assert!(!self.blocked_positions.contains_key(&pos), "Tried to schedule invalid move");

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

    fn solve_distant_move(&mut self, creep: &CheckedID<Creep>, target: &MoveTarget) {
        if self.try_move_by_path(creep, target) { return }

        let room_adj_blocked = adjacent_positions(creep.pos())
            .filter(|pos| self.position_priority(*pos).is_none())
            .into_grouping_map_by(|pos| pos.room_name())
            .collect::<Vec<_>>();

        let options = SearchOptions::default()
            .plain_cost(2).swamp_cost(10)
            .room_callback(|room| {
                let mut cm = self.get_costmatrix(room);

                if let Some(changes) = room_adj_blocked.get(&room) {
                    for pos in changes {
                        cm.set_xy(pos.xy(), 255);
                    }
                }

                MultiRoomCostResult::CostMatrix(cm)
            });

        let path = pathfinder::search(creep.pos(), target.target, target.range, Some(options)).path();
        let mut path = VecDeque::from_iter(path);
        path.push_front(creep.pos());

        self.mem.store_path(creep, target.clone(), path);
        if !self.try_move_by_path(creep, target) {
            self.give_creep_action(creep, CreepAction::Stay);
        }
    }

    fn get_costmatrix(&mut self, room: RoomName) -> CostMatrix {
        self.costmatrix_cache.entry(room)
            .or_insert_with(|| {
                let mut cm = CostMatrix::new();
                let Some(room) = game::rooms().get(room) else { return cm };

                for structure in room.find(find::STRUCTURES, None) {
                    if structure.structure_type().is_obstacle() {
                        cm.set_xy(structure.pos().xy(), 255);
                    } else if matches!(structure.structure_type(), StructureType::Road) {
                        cm.set_xy(structure.pos().xy(), 1);
                    }
                }

                for creep in room.find(find::CREEPS, None) {
                    if !creep.my() {
                        cm.set_xy(creep.pos().xy(), 255);
                    }
                }

                cm
            }).clone()
    }

    fn try_move_by_path(&mut self, creep: &CheckedID<Creep>, target: &MoveTarget) -> bool {
        let dir = self.mem.get_path_direction(creep, target);
        if let Some(dir) = dir && self.position_priority(creep.pos() + dir).is_some() {
            self.give_creep_action(creep, CreepAction::Move { dir });
            return true;
        }

        false
    }

    fn solve_local_move(&mut self, creep: &CheckedID<Creep>, target: &MoveTarget, must_move: bool) {
        if !must_move && self.position_priority(creep.pos()).is_some() {
            self.give_creep_action(creep, CreepAction::Stay);
            return;
        }

        let next_pos = Self::best_by_priority(
            adjacent_positions(creep.pos()),
            |pos| {
                let pos_prio = self.position_priority(*pos)?;
                let target_prio = u8::from(target.in_range(*pos));
                Some((target_prio, pos_prio))
            });

        let next_pos = next_pos.unwrap_or(creep.pos());
        let dir = creep.pos().get_direction_to(next_pos);

        if let Some(dir) = dir {
            self.give_creep_action(creep, CreepAction::Move { dir });
        } else {
            self.give_creep_action(creep, CreepAction::Stay);
        }
    }

    fn solve_free(&mut self, creep: &CheckedID<Creep>) {
        self.solve_local_move(
            creep, 
            &MoveTarget { target: creep.pos(), range: 1 }, 
            false
        );
    }

    fn position_priority(&self, pos: Position) -> Option<(u8, u8)> {
        if self.blocked_positions.contains_key(&pos) { return None }
        if RoomTerrain::new(pos.room_name()).unwrap().get_xy(pos.xy()) == Terrain::Wall { return None }

        let structures = pos.look_for(look::STRUCTURES).unwrap_or_default();
        if structures.iter().any(|structure| structure.structure_type().is_obstacle()) { return None }
        let is_road = structures.iter().any(|structure| matches!(structure.structure_type(), StructureType::Road));

        let creeps = pos.look_for(look::CREEPS).unwrap_or_default();
        let (my_creeps, enemy_creeps) = creeps.into_iter().partition::<Vec<_>, _>(Creep::my);
        if !enemy_creeps.is_empty() { return None }

        let terrain = RoomTerrain::new(pos.room_name()).unwrap().get_xy(pos.xy());
        let terrain_prio = if is_road { 2_u8 } else {
                match terrain {
                    Terrain::Plain => 1,
                    Terrain::Swamp => 0,
                    Terrain::Wall => return None,
                }
            };

        assert!(my_creeps.len() <= 1);
        let Some(other) = my_creeps.first().and_then(TryGetCheckedID::try_check_id) else { 
            return Some((3, terrain_prio)) 
        };

        match self.creeps.creeps.get(&other).unwrap() {
            CreepConstraint::Stay => None,
            CreepConstraint::Follow(_) 
            | CreepConstraint::Move { .. } => Some((1, terrain_prio)),
            CreepConstraint::Free => Some((0, terrain_prio)),
        }
    }

    fn best_by_priority<A, K: Ord + Copy>(iter: impl Iterator<Item = A>, prio: impl Fn(&A) -> Option<K>) -> Option<A> {
        iter.filter_map(|x| prio(&x).map(|prio| (prio, x)))
            .max_by_key(|(prio, _)| *prio)
            .map(|(_, x)| x)
    }

    fn execute(self) {
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
    fn get_path_direction(&mut self, creep: &CheckedID<Creep>, target: &MoveTarget) -> Option<Direction> {
        let path = self.paths.get_mut(creep)?;
        if path.target == *target && game::time() < path.cache_time + 5 {
            while path.path.front().is_some_and(|pos| *pos != creep.pos()) {
                path.path.pop_front();
            }

            let next_pos = *path.path.get(1)?;
            Some(creep.pos().get_direction_to(next_pos).unwrap())
        } else {
            self.paths.remove(creep);
            None
        }
    }

    fn store_path(&mut self, creep: &CheckedID<Creep>, target: MoveTarget, path: VecDeque<Position>) {
        self.paths.insert(
            creep.clone(), 
            CachedPath {
                cache_time: game::time(),
                path,
                target
            }
        );
    }
}