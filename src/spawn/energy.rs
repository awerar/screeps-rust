use itertools::Itertools;
use screeps::{SPAWN_ENERGY_CAPACITY, Source, StructureExtension, StructureSpawn};

use crate::{creeps::CreepData, domain_traits::{CreepId, EnergyStoreAccessors, ObjectId}, memory::Memory};

enum EnergyPoolType {
    Finite,
    RefilledTo(u32)
}

impl EnergyPoolType {
    fn refilled_if(cond: bool, capacity: u32) -> Self {
        if cond { Self::RefilledTo(capacity) }
        else { EnergyPoolType::Finite }
    }
}

pub struct EnergyPool {
    pub current: u32,
    ty: EnergyPoolType
}

impl EnergyPool {
    fn new(current: u32, ty: EnergyPoolType) -> Self {
        Self { current, ty }
    }

    pub fn future_energy(&self) -> u32 {
        match &self.ty {
            EnergyPoolType::Finite => self.current,
            EnergyPoolType::RefilledTo(capacity) => (*capacity).max(self.current),
        }
    }

    pub fn is_refilled(&self) -> bool {
        matches!(&self.ty, EnergyPoolType::RefilledTo(_))
    }
}

enum SpawnState {
    Free,
    Blocked,
    Spawning(CreepId, CreepData)
}

pub enum ColonySpawnType {
    Central,
    Source(ObjectId<Source>)
}

pub struct ColonySpawn {
    pub spawn: StructureSpawn,
    state: SpawnState,
    ty: ColonySpawnType,

    pub energy: EnergyPool,
}

impl ColonySpawn {
    pub fn new(spawn: StructureSpawn, ty: ColonySpawnType, gets_energy: bool) -> Self {
        Self {
            energy: EnergyPool::new(
                spawn.used_energy_capacity(),
                EnergyPoolType::refilled_if(gets_energy, SPAWN_ENERGY_CAPACITY)
            ),
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

    pub fn block(&mut self) {
        self.state = SpawnState::Blocked;
    }

    pub fn begin_spawning(&mut self, id: CreepId, data: CreepData) {
        self.state = SpawnState::Spawning(id, data);
    }

    pub fn gather_new_creeps(self, mem: &mut Memory) {
        if let SpawnState::Spawning(id, creep_data) = self.state  {
            mem.creeps.insert(id, creep_data);
        }
    }
}

pub struct ExtensionGroup {
    extensions_left: Vec<StructureExtension>,
    energy: EnergyPool,
    central: bool
}

impl ExtensionGroup {
    pub fn new(extensions: Vec<StructureExtension>, refilled: bool, central: bool) -> Self {
        Self {
            energy: EnergyPool::new(
                extensions.iter().map(EnergyStoreAccessors::used_energy_capacity).sum::<u32>(),
                EnergyPoolType::refilled_if(
                    refilled,
                    extensions.iter().map(EnergyStoreAccessors::energy_capacity).sum::<u32>()
                )
            ),
            extensions_left: extensions.into_iter()
                .filter(|extension| extension.used_energy_capacity() > 0)
                .rev()
                .collect_vec(),
            central
        }
    }

    pub fn allocate(&mut self, mut amount: u32) -> Vec<StructureExtension> {
        assert!(self.energy.current >= amount);

        let mut extensions = Vec::new();
        while amount > 0 {
            let extension = self.extensions_left.pop().unwrap();
            amount = amount.saturating_sub(extension.used_energy_capacity());
            self.energy.current -= extension.used_energy_capacity();

            extensions.push(extension);
        }

        extensions
    }

    pub fn reserve_future(&mut self, amount: u32) {
        assert!(self.energy.future_energy() >= amount);

        if !self.energy.is_refilled() {
            self.allocate(amount);
        }
    }
}

pub struct ColonyExtensions(Vec<ExtensionGroup>);

impl ColonyExtensions {
    pub fn new(groups: Vec<ExtensionGroup>) -> Self {
        Self(groups)
    }

    pub fn energy(&self) -> u32 {
        self.0.iter().map(|group| group.energy.current).sum()
    }

    pub fn future_energy(&self) -> u32 {
        self.0.iter().map(|group| group.energy.future_energy()).sum()
    }

    pub fn allocate(&mut self, mut amount: u32) -> Vec<StructureExtension> {
        assert!(self.energy() >= amount);

        let mut extensions = Vec::new();
        for group in self.0.iter_mut().sorted_by_key(|group| !group.energy.is_refilled()) {
            if amount == 0 { break; }

            let group_amount = amount.min(group.energy.current);
            amount -= group_amount;

            extensions.extend(group.allocate(group_amount));
        }

        extensions
    }

    pub fn reserve_future(&mut self, mut amount: u32) {
        assert!(self.future_energy() >= amount);

        for group in self.0.iter_mut().sorted_by_key(|group| !group.energy.is_refilled()) {
            if amount == 0 { return }

            let group_amount = amount.min(group.energy.future_energy());
            amount -= group_amount;

            group.reserve_future(group_amount);
        }
    }
}
