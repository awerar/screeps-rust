use std::{hash::Hash, ops::Deref};

use screeps::{HasId, ObjectId, Position, Spawning, StructureSpawn};
use serde::{Deserialize, Serialize};

pub mod requests;
mod simplifier;
mod solver;

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
            spawn: self.spawn.clone(), 
            spawning: self.spawn.resolve().unwrap().spawning().unwrap() 
        }
    }
}

impl Deref for SpawningID {
    type Target = Spawning;

    fn deref(&self) -> &Self::Target {
        &self.spawning
    }
}