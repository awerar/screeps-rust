use std::{cell::RefCell, collections::{HashMap, HashSet, VecDeque}, hash::Hash, ops::Deref};

use screeps::{Creep, HasPosition, ObjectId, Position, SharedCreepProperties, Spawning, StructureSpawn};
use serde::{Deserialize, Serialize};
use crate::{commands::{Command, pop_command}, safeid::{GetSafeID, SafeID, deserialize_prune_hashmap_keys}};

pub mod requests;
mod simplifier;
mod solver;

thread_local! {
    static SELECTED: RefCell<HashSet<ObjectId<Creep>>> = RefCell::new(HashSet::new());
}

fn has_selected(creep: &SafeID<Creep>) -> bool {
    SELECTED.with_borrow_mut(|selected| {
        if pop_command(Command::VisualizeMovement { creep: creep.name() }) {
            selected.insert(creep.id);
        }

        selected.contains(&creep.id)
    })
}

#[derive(Serialize, Deserialize, Default)]
pub struct MovementMemory {
    #[serde(deserialize_with = "deserialize_prune_hashmap_keys")]
    paths: HashMap<SafeID<Creep>, CachedPath>
}

#[derive(Serialize, Deserialize)]
struct CachedPath {
    path: VecDeque<Position>,
    target: MoveTarget,
    cache_time: u32
}

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
    spawn: SafeID<StructureSpawn>,
    spawning: Spawning
}

impl SpawningID {
    fn new(spawn: &StructureSpawn) -> Option<Self> {
        Some(Self {
            spawn: spawn.safe_id(),
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
            spawn: self.spawn.clone(), 
            spawning: self.spawn.spawning().unwrap() 
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
        self.spawn.pos()
    }
}