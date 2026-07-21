use std::{cell::RefCell, collections::{HashMap, HashSet, VecDeque}, hash::Hash, ops::Deref};

use screeps::{Creep, Direction, HasPosition, Position, SharedCreepProperties, Spawning, StructureSpawn};
use serde::{Deserialize, Serialize};
use crate::{check::{TriviallyChecked, filter_check_any_key_map}, commands::{Command, pop_command}, domain_traits::{CreepId, HasId, ObjectId, ResolvableId}};

pub mod requests;
mod simplifier;
mod solver;

thread_local! {
    static SELECTED: RefCell<HashSet<screeps::ObjectId<Creep>>> = RefCell::new(HashSet::new());
}

fn has_selected(creep: &Creep) -> bool {
    let Some(id) = screeps::MaybeHasId::try_id(creep) else { return false };

    SELECTED.with_borrow_mut(|selected| {
        if pop_command(Command::VisualizeMovement { creep: creep.name() }) {
            selected.insert(id);
        }

        selected.contains(&id)
    })
}

#[derive(Serialize, Deserialize, Default)]
pub struct MovementMemory {
    #[serde(with = "filter_check_any_key_map")]
    paths: HashMap<CreepId, CachedPath>,

    #[serde(with = "filter_check_any_key_map")]
    pub spawning_directions: HashMap<CreepId, Vec<Direction>>
}

#[derive(Serialize, Deserialize)]
struct CachedPath {
    path: VecDeque<Position>,
    target: MoveTarget,
    cache_time: u32
}

impl TriviallyChecked for CachedPath {}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
struct MoveTarget {
    pub target: Position, 
    pub range: u32
}

impl MoveTarget {
    pub fn in_range(&self, pos: Position) -> bool {
        pos.get_range_to(self.target) <= self.range
    }
}

struct SpawningID {
    spawn: ObjectId<StructureSpawn>,
    spawning: Spawning
}

impl SpawningID {
    fn new(spawn: &StructureSpawn) -> Option<Self> {
        Some(Self {
            spawn: spawn.id(),
            spawning: spawn.spawning()?,
        })
    }
}

impl Hash for SpawningID {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.spawn.hash(state);
    }
}

impl PartialEq for SpawningID {
    fn eq(&self, other: &Self) -> bool {
        self.spawn == other.spawn
    }
}

impl Eq for SpawningID {}

impl Clone for SpawningID {
    fn clone(&self) -> Self {
        Self { 
            spawn: self.spawn, 
            spawning: self.spawn.resolve().spawning().unwrap() 
        }
    }
}

impl Deref for SpawningID {
    type Target = Spawning;

    fn deref(&self) -> &Self::Target {
        &self.spawning
    }
}

impl HasPosition for SpawningID {
    fn pos(&self) -> Position {
        self.spawn.resolve().pos()
    }
}