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

use crate::{colony::ColonyView, domain_traits::HasId, memory::Memory, spawn::scheduler::{budget::ColonyBudget, creeps::KnownCreeps, energy::{ColonyEnergy, EnergyGroup, EnergyStructure}, spawns::{ColonySpawn, ColonySpawnIterator, ColonySpawnType}, syndrome::ColonySyndrome}, statemachine::{Flow, OrBreak}};

pub use global::{ColonySchedulerMut, ColonyScheduler, SpawnScheduler};
pub use roles::RoleSelector;

pub struct ColonySchedulerState {
    spawns: Vec<ColonySpawn>,
    energy: ColonyEnergy,
    syndrome: ColonySyndrome,
    budget: ColonyBudget
}

impl ColonySchedulerState {
    pub fn new(colony: &ColonyView<'_>, creeps: &KnownCreeps) -> Self {
        let syndrome = ColonySyndrome::new(creeps.of_home(colony.name), colony);

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
            syndrome.any_excavating_excavators && syndrome.any_trucks
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

        let mut spawns = Vec::new();

        spawns.extend(
            colony.plan.center.spawn.resolve()
                .map(|spawn| {
                    ColonySpawn::new(
                        spawn,
                        ColonySpawnType::Central 
                    )
                })
        );
        
        spawns.extend(
            colony.plan.sources.iter()
                .filter_map(|(source, plan)| {
                    Some(ColonySpawn::new(
                        plan.spawn.resolve()?,
                        ColonySpawnType::Source(source.resolve()?.id(), plan.spawn_direction)
                    ))
                })
        );

        Self {
            spawns,
            energy: ColonyEnergy::new(groups),
            syndrome,
            budget: ColonyBudget::new(colony, creeps)
        }
    }

    fn free_spawns(&self) -> impl Iterator<Item = &ColonySpawn> {
        self.spawns.iter().filter(|spawn| spawn.is_free())
    }

    fn has_free(&self) -> bool {
        self.free_spawns().next().is_some()
    }

    fn gather_new_creeps(self, mem: &mut Memory) {
        for spawn in self.spawns {
            spawn.gather_new_creeps(mem);
        }
    }
}

fn default_select(iter: ColonySpawnIterator<'_>) -> Flow<usize> {
    iter.max_by_key(|(_, spawn)| spawn.is_central()).map(|(ix, _)| ix).or_break()
}
