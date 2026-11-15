mod budget;
mod creeps;
mod energy;
mod global;
mod prototype;
mod roles;
mod spawns;
mod syndrome;

use itertools::Itertools;
use screeps::HasPosition;

use crate::{colony::ColonyView, domain_traits::HasId, memory::Memory, spawn::scheduler::{budget::ColonyBudget, creeps::KnownCreeps, energy::{ColonyEnergy, EnergyGroup, EnergyStructure}, spawns::{ColonySpawn, ColonySpawnType, ColonySpawns}, syndrome::ColonySyndrome}};

pub use global::{ColonySchedulerRef, ColonySchedulerMut, ColonyScheduler, SpawnScheduler};
pub use roles::RoleSelector;

pub struct ColonySchedulerState {
    spawns: ColonySpawns,
    energy: ColonyEnergy,
    syndrome: ColonySyndrome,
    budget: ColonyBudget
}

impl ColonySchedulerState {
    pub fn new(colony: &ColonyView<'_>, creeps: &KnownCreeps) -> Self {
        let syndrome = ColonySyndrome::new(creeps.of_home(colony.name), colony);

        let any_ok_excavator = colony.plan.sources.keys()
            .filter_map(|source| source.resolve().map(|x| x.id()))
            .any(|source| !syndrome.excavators.contains_key(&source));

        let mut groups = Vec::new();

        groups.push(EnergyGroup::new(
            colony.plan.center.spawn.resolve()
                .map(|spawn| EnergyStructure::Spawn(spawn.id()))
                .into_iter()
                .chain(
                    colony.plan.center.extensions.resolve().into_iter()
                    .sorted_by_cached_key(|extension| extension.pos().get_range_to(colony.center))
                    .map(|ext| EnergyStructure::Extension(ext.id()))
                ).collect(),
            any_ok_excavator && syndrome.any_trucks
        ));

        for (source, source_plan) in &colony.plan.sources {
            let Some(source) = source.resolve().map(|src| src.id()) else { continue; };

            groups.push(EnergyGroup::new(
                source_plan.spawn.resolve()
                    .map(|spawn| EnergyStructure::Spawn(spawn.id()))
                    .into_iter()
                    .chain(
                        source_plan.extensions.resolve().into_iter()
                            .map(|ext| EnergyStructure::Extension(ext.id())))
                    .collect(),
                !syndrome.excavators.contains_key(&source)
            ));
        }

        let spawns = colony.plan.center.spawn.resolve()
            .map(|spawn| (spawn.id(), ColonySpawn::new(&spawn, ColonySpawnType::Central)))
            .into_iter()
            .chain(
                colony.plan.sources.iter()
                    .filter_map(|(source, plan)| {
                        let spawn = plan.spawn.resolve()?;
                        Some((spawn.id(), ColonySpawn::new(&spawn, ColonySpawnType::Source(source.resolve()?.id(), plan.spawn_direction))))
                    })
            )
            .collect();

        Self {
            spawns: ColonySpawns::new(spawns),
            energy: ColonyEnergy::new(groups),
            syndrome,
            budget: ColonyBudget::new(colony, creeps)
        }
    }

    fn gather_new_creeps(self, mem: &mut Memory) {
        for spawn in self.spawns.into_values() {
            spawn.gather_new_creeps(mem);
        }
    }
}
