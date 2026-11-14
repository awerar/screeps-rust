use std::collections::HashMap;

use screeps::{Direction, Source, StructureSpawn};

use crate::{creeps::CreepData, domain_traits::{CreepId, ObjectId}, memory::Memory};

enum SpawnState {
    Free,
    Blocked,
    Spawning(CreepId, CreepData, Vec<Direction>)
}

pub enum ColonySpawnType {
    Central,
    Source(ObjectId<Source>, Direction)
}

pub struct ColonySpawn {
    state: SpawnState,
    ty: ColonySpawnType
}

impl ColonySpawn {
    pub fn new(spawn: &StructureSpawn, ty: ColonySpawnType) -> Self {
        Self {
            state: if spawn.spawning().is_some() { SpawnState::Blocked } else { SpawnState::Free },
            ty
        }
    }

    pub fn is_free(&self) -> bool {
        matches!(self.state, SpawnState::Free)
    }

    pub fn is_central(&self) -> bool {
        matches!(self.ty, ColonySpawnType::Central)
    }

    pub fn is_source_spawn(&self, source: &ObjectId<Source>) -> bool {
        matches!(self.ty, ColonySpawnType::Source(source2, _) if *source == source2)
    }

    pub fn source_direction(&self) -> Option<Direction> {
        if let ColonySpawnType::Source(_, dir) = &self.ty { 
            Some(*dir) 
        } else { 
            None 
        }
    }

    pub fn block(&mut self) {
        self.state = SpawnState::Blocked;
    }

    pub fn begin_spawning(&mut self, id: CreepId, data: CreepData, dirs: Vec<Direction>) {
        self.state = SpawnState::Spawning(id, data, dirs);
    }

    pub fn gather_new_creeps(self, mem: &mut Memory) {
        if let SpawnState::Spawning(id, creep_data, dirs) = self.state  {
            mem.creeps.insert(id.clone(), creep_data);
            mem.movement.spawning_directions.insert(id, dirs);
        }
    }
}

pub struct ColonySpawns(HashMap<ObjectId<StructureSpawn>, ColonySpawn>);

impl ColonySpawns {
    pub fn new(spawns: HashMap<ObjectId<StructureSpawn>, ColonySpawn>) -> Self {
        Self(spawns)
    }

    pub fn get_mut(&mut self, id: ObjectId<StructureSpawn>) -> &mut ColonySpawn {
        self.0.get_mut(&id).expect("Spawn should belong to this colony")
    }

    pub fn free(&self) -> impl Iterator<Item = (ObjectId<StructureSpawn>, &ColonySpawn)> {
        self.0.iter()
            .filter(|(_, spawn)| spawn.is_free())
            .map(|(id, spawn)| (*id, spawn))
    }

    pub fn any_free(&self) -> bool {
        self.0.values().any(ColonySpawn::is_free)
    }

    pub fn into_values(self) -> impl Iterator<Item = ColonySpawn> {
        self.0.into_values()
    }
}
