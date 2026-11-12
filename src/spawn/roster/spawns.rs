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
    pub spawn: StructureSpawn,
    state: SpawnState,
    ty: ColonySpawnType
}

impl ColonySpawn {
    pub fn new(spawn: StructureSpawn, ty: ColonySpawnType) -> Self {
        Self {
            state: if spawn.spawning().is_some() { SpawnState::Blocked } else { SpawnState::Free },
            spawn,
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

pub struct ColonySpawnIterator<'a> {
    index: usize,
    spawns: &'a [ColonySpawn]
}

impl<'a> ColonySpawnIterator<'a> {
    pub fn new(spawns: &'a [ColonySpawn]) -> Self {
        Self { index: 0, spawns }
    }
}

impl<'a> Iterator for ColonySpawnIterator<'a> {
    type Item = (usize, &'a ColonySpawn);

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(spawn) = self.spawns.get(self.index) {
            let index = self.index;
            self.index += 1;

            if !spawn.is_free() { continue; }

            return Some((index, spawn))
        }

        None
    }
}
