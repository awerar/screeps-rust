use itertools::Itertools;
use screeps::{Structure, StructureExtension, StructureSpawn};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{check::{Check, CheckFrom}, domain_traits::{EnergyStoreAccessors, HasStore, IdResolutionError, ObjectId, ResolvableId}, ids::{CheckState, Checked, Unchecked}};

#[derive(Serialize, Deserialize, Clone, Copy)]
pub enum EnergyStructure<S: CheckState = Checked> {
    Spawn(ObjectId<StructureSpawn, S>),
    Extension(ObjectId<StructureExtension, S>)
}

impl HasStore for EnergyStructure {
    fn store(&self) -> screeps::Store {
        match self {
            EnergyStructure::Spawn(id) => id.resolve().store(),
            EnergyStructure::Extension(id) => id.resolve().store(),
        }
    }
}

#[derive(Debug, Error)]
pub enum EnergyStructureCheckError {
    #[error(transparent)] Spawn(#[from] IdResolutionError<StructureSpawn>),
    #[error(transparent)] Extension(#[from] IdResolutionError<StructureExtension>)
}

impl CheckFrom for EnergyStructure {
    type Unchecked = EnergyStructure<Unchecked>;
    type Err = EnergyStructureCheckError;

    fn check_from(uc: Self::Unchecked) -> Result<Self, Self::Err> {
        Ok(match uc {
            EnergyStructure::Spawn(id) => Self::Spawn(id.check()?),
            EnergyStructure::Extension(id) => Self::Extension(id.check()?),
        })
    }
}

impl From<EnergyStructure> for Structure {
    fn from(value: EnergyStructure) -> Self {
        match value {
            EnergyStructure::Extension(id) => id.resolve().into(),
            EnergyStructure::Spawn(id) => id.resolve().into()
        }
    }
}

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

pub struct EnergyGroup {
    structures_left: Vec<EnergyStructure>,
    energy: EnergyPool
}

impl EnergyGroup {
    pub fn new(structures: Vec<EnergyStructure>, refilled: bool) -> Self {
        Self {
            energy: EnergyPool::new(
                structures.iter().map(EnergyStoreAccessors::used_energy_capacity).sum::<u32>(),
                EnergyPoolType::refilled_if(
                    refilled,
                    structures.iter().map(EnergyStoreAccessors::energy_capacity).sum::<u32>()
                )
            ),
            structures_left: structures.into_iter()
                .filter(|extension| extension.used_energy_capacity() > 0)
                .rev()
                .collect_vec()
        }
    }

    pub fn allocate(&mut self, mut amount: u32) -> Vec<EnergyStructure> {
        assert!(self.energy.current >= amount);

        let mut structures = Vec::new();
        while amount > 0 {
            let extension = self.structures_left.pop().unwrap();
            amount = amount.saturating_sub(extension.used_energy_capacity());
            self.energy.current -= extension.used_energy_capacity();

            structures.push(extension);
        }

        structures
    }

    pub fn reserve_future(&mut self, amount: u32) {
        assert!(self.energy.future_energy() >= amount);

        if !self.energy.is_refilled() {
            self.allocate(amount);
        }
    }
}

pub struct ColonyEnergy(Vec<EnergyGroup>);

impl ColonyEnergy {
    pub fn new(groups: Vec<EnergyGroup>) -> Self {
        Self(groups)
    }

    pub fn energy(&self) -> u32 {
        self.0.iter().map(|group| group.energy.current).sum()
    }

    pub fn future_energy(&self) -> u32 {
        self.0.iter().map(|group| group.energy.future_energy()).sum()
    }

    pub fn allocate(&mut self, mut amount: u32) -> Vec<EnergyStructure> {
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
