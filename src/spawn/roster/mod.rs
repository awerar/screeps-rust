mod budget;
mod creeps;
mod energy;
mod global;
mod prototype;
mod roles;
mod spawns;
mod syndrome;

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use itertools::Itertools;
use screeps::{Direction, HasPosition, RoomName, SpawnOptions, Structure, action_error_codes::SpawnCreepErrorCode, game};
use thiserror::Error;

use crate::{colony::ColonyView, creeps::{CreepData, CreepRole, body::Body}, domain_traits::{CreepId, HasId}, memory::Memory, names::UsedNames, spawn::roster::{budget::ColonyBudget, creeps::{ColonyCreeps, GlobalCreeps}, energy::{ColonyEnergy, EnergyGroup, EnergyStructure}, prototype::AbsolutePrototype, spawns::{ColonySpawn, ColonySpawnIterator, ColonySpawnType}, syndrome::ColonySyndrome}};

pub use global::Rosters;
pub use roles::RoleSelector;

pub type SharedUsedNames = Rc<RefCell<UsedNames>>;
pub type SharedColonyBudgets = Rc<RefCell<HashMap<RoomName, ColonyBudget>>>;

pub struct ColonyRoster {
    name: RoomName,

    spawns: Vec<ColonySpawn>,
    energy: ColonyEnergy,
    local_creeps: ColonyCreeps,

    syndrome: ColonySyndrome,

    budgets: SharedColonyBudgets,
    names: SharedUsedNames
}

pub enum ScheduleDecision {
    Scheduled(CreepId, AbsolutePrototype),
    WaitingForEnergy,
    WaitingForSpawn
}

#[derive(Debug, Error)]
pub enum ColonyScheduleError {
    #[error(transparent)] SpawnError(#[from] SpawnCreepErrorCode),
    #[error("Not enough energy")] NotEnoughEnergy
}

pub type ColonyScheduleResult = Result<ScheduleDecision, ColonyScheduleError>;

impl ColonyRoster {
    pub fn new(colony: &ColonyView<'_>, mem: &Memory, names: SharedUsedNames, budgets: SharedColonyBudgets, global_creeps: &GlobalCreeps) -> Self {
        let local_creeps = ColonyCreeps::new(colony.name, mem);
        let syndrome = ColonySyndrome::new(&local_creeps, colony);

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

        budgets.borrow_mut().insert(colony.name, ColonyBudget::new(colony, global_creeps));

        Self {
            spawns,
            energy: ColonyEnergy::new(groups),
            names,
            name: colony.name,
            local_creeps,
            budgets,
            syndrome
        }
    }

    fn free_spawns(&self) -> impl Iterator<Item = &ColonySpawn> {
        self.spawns.iter().filter(|spawn| spawn.is_free())
    }

    pub fn has_free(&self) -> bool {
        self.free_spawns().next().is_some()
    }

    pub fn local_creeps(&self) -> &ColonyCreeps {
        &self.local_creeps
    }

    pub fn syndrome(&self) -> &ColonySyndrome {
        &self.syndrome
    }

    pub fn full_budget(&self) -> f32 {
        self.budgets.borrow()[&self.name].full()
    }

    pub fn sustainable_budget(&self) -> f32 {
        self.budgets.borrow()[&self.name].sustainable()  
    }

    pub fn income(&self) -> f32 {
        self.budgets.borrow()[&self.name].income()
    }

    pub fn future_energy(&self) -> u32 {
        self.energy.future_energy()
    }

    pub fn room(&self) -> RoomName {
        self.name
    }

    // Not meant to be used by user
    fn schedule_selected_absolute<S>(&mut self, select: S, body: Body, role: CreepRole, home: RoomName) -> ColonyScheduleResult
    where
        S: for<'b> FnOnce(ColonySpawnIterator<'b>) -> Option<usize>
    {
        
        let Some(choice) = select(ColonySpawnIterator::new(&self.spawns)) else { return Ok(ScheduleDecision::WaitingForSpawn) };
        let spawn = self.spawns.get_mut(choice).expect("Spawn selection should return a valid index");
        
        let proto = AbsolutePrototype::new(body, role, home, spawn.spawn.room().unwrap().name());

        assert!(spawn.is_free());

        let cost = proto.body().energy_required();

        if cost > self.energy.future_energy() { return Err(ColonyScheduleError::NotEnoughEnergy) }
        if cost > self.energy.energy() {
            spawn.block();
            self.energy.reserve_future(cost);
            return Ok(ScheduleDecision::WaitingForEnergy)
        }

        let structures = self.energy.allocate(cost);

        let dirs = if let Some(dir) = spawn.source_direction() {
            if let CreepRole::Excavator(source) = proto.role() && spawn.is_source_spawn(source) {
                vec![dir]
            } else {
                Direction::iter().filter(|dir2| **dir2 != dir).copied().collect_vec()
            }
        } else {
            Direction::iter().copied().collect_vec()
        };

        let name = self.names.borrow_mut().generate_new(proto.role());
        spawn.spawn.spawn_creep_with_options(
            proto.body().parts(),
            &name,
            &SpawnOptions::new()
                .energy_structures(structures.into_iter().map_into::<Structure>())
                .directions(&dirs)
        )?;

        for budget in self.budgets.borrow_mut().values_mut() {
            budget.add_creep(&proto);
        }

        let id = game::creeps().get(name).unwrap().id();
        spawn.begin_spawning(id.clone(), CreepData { role: proto.role().clone(), home: proto.home(), spawn_room: self.name }, dirs);

        Ok(ScheduleDecision::Scheduled(id, proto))
    }

    pub fn schedule_selected<S>(&mut self, select: S, body: Body, role: CreepRole) -> ColonyScheduleResult
    where
        S: for<'a> FnOnce(ColonySpawnIterator<'a>) -> Option<usize>
    {
        let home = self.name;
        let result = self.schedule_selected_absolute(select, body, role, home);

        if let Ok(ScheduleDecision::Scheduled(id, proto)) = &result {
            self.local_creeps.0.insert(id.clone(), proto.relative().clone());
        }

        result
    }

    pub fn default_select(iter: ColonySpawnIterator<'_>) -> Option<usize> {
        iter.max_by_key(|(_, spawn)| spawn.is_central()).map(|(ix, _)| ix)
    }

    pub fn schedule(&mut self, body: Body, role: CreepRole) -> ColonyScheduleResult {
        self.schedule_selected(Self::default_select, body, role)
    }

    fn gather_new_creeps(self, mem: &mut Memory) {
        for spawn in self.spawns {
            spawn.gather_new_creeps(mem);
        }
    }
}
