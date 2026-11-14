use std::{collections::HashMap, ops::Deref};

use anyhow::anyhow;
use itertools::Itertools;
use screeps::{Direction, SpawnOptions, Structure, StructureSpawn, game};

use crate::{colony::ColonyName, creeps::{CreepData, CreepRole, body::Body}, domain_traits::{HasId, ObjectId, ResolvableId}, memory::Memory, names::UsedNames, spawn::scheduler::{ColonySchedulerState, creeps::{KnownCreeps, HomeCreeps}, prototype::AbsolutePrototype, spawns::ColonySpawns, syndrome::ColonySyndrome}, statemachine::{Flow, OrBreak, break_if}};

struct ColonyStates(HashMap<ColonyName, ColonySchedulerState>);

impl ColonyStates {
    fn get(&self, name: ColonyName) -> &ColonySchedulerState {
        self.0.get(&name).expect("SpawnScheduler should contain state for each colony")
    }

    fn get_mut(&mut self, name: ColonyName) -> &mut ColonySchedulerState {
        self.0.get_mut(&name).expect("SpawnScheduler should contain state for each colony")
    }

    fn names(&self) -> impl Iterator<Item = ColonyName> {
        self.0.keys().copied()
    }

    fn values_mut(&mut self) -> impl Iterator<Item = &mut ColonySchedulerState> {
        self.0.values_mut()
    }

    fn into_values(self) -> impl Iterator<Item = ColonySchedulerState> {
        self.0.into_values()
    }
}

pub struct SpawnScheduler {
    colonies: ColonyStates,
    creeps: KnownCreeps,
    used_names: UsedNames
}

impl SpawnScheduler {
    pub fn new(mem: &Memory) -> Self {
        let creeps = KnownCreeps::new(mem);

        Self {
            colonies: ColonyStates(mem.colonies.view_all()
                .map(|colony| (colony.name, ColonySchedulerState::new(&colony, &creeps)))
                .collect()),
            creeps,
            used_names: UsedNames::new()
        }
    }

    pub fn colonies(&self) -> Vec<ColonyName> {
        self.colonies.names().collect()
    }

    pub fn iter(&self) -> impl Iterator<Item = ColonySchedulerRef<'_>> {
        self.colonies.names().map(move |name| ColonyScheduler { scheduler: self, name })
    }

    pub fn get(&self, name: ColonyName) -> ColonySchedulerRef<'_> {
        ColonyScheduler { scheduler: self, name }
    }

    pub fn get_mut(&mut self, name: ColonyName) -> ColonySchedulerMut<'_> {
        ColonyScheduler { scheduler: self, name }
    }

    pub fn creeps(&self) -> &KnownCreeps {
        &self.creeps
    }

    pub fn gather_new_creeps(self, mem: &mut Memory) {
        for colony in self.colonies.into_values() {
            colony.gather_new_creeps(mem);
        }
    }
}

pub struct ColonyScheduler<R> {
    scheduler: R,
    name: ColonyName
}

pub type ColonySchedulerRef<'a> = ColonyScheduler<&'a SpawnScheduler>;
pub type ColonySchedulerMut<'a> = ColonyScheduler<&'a mut SpawnScheduler>;

impl<R: Deref<Target = SpawnScheduler>> ColonyScheduler<R> {
    fn colony(&self) -> &ColonySchedulerState {
        self.scheduler.colonies.get(self.name)
    }

    pub fn name(&self) -> ColonyName {
        self.name
    }

    pub fn spawns(&self) -> &ColonySpawns {
        &self.colony().spawns
    }

    pub fn is_available(&self) -> bool {
        !self.syndrome().any_problems() && self.spawns().any_free()
    }

    pub fn syndrome(&self) -> &ColonySyndrome {
        &self.colony().syndrome
    }

    pub fn future_energy(&self) -> u32 {
        self.colony().energy.future_energy()
    }

    pub fn home_creeps(&self) -> HomeCreeps<'_> {
        self.scheduler.creeps.of_home(self.name)
    }

    pub fn full_budget(&self) -> f32 {
        self.colony().budget.full()
    }

    pub fn sustainable_budget(&self) -> f32 {
        self.colony().budget.sustainable()
    }

    pub fn income(&self) -> f32 {
        self.colony().budget.income()
    }
}

impl ColonySchedulerMut<'_> {
    fn schedule_selected_for(&mut self, spawn_id: ObjectId<StructureSpawn>, body: Body, role: CreepRole, home: ColonyName) -> Flow {
        let colony = self.scheduler.colonies.get_mut(self.name);
        let spawn = colony.spawns.get_mut(spawn_id);

        break_if(!spawn.is_free())?;

        let proto = AbsolutePrototype::new(body, role, home.into(), self.name.room_name());

        let cost = proto.body().energy_required();

        if cost > colony.energy.future_energy() { return Flow::Fail(anyhow!("Not enough energy")) }
        if cost > colony.energy.energy() {
            spawn.block();
            colony.energy.reserve_future(cost);
            return Flow::Break
        }

        let structures = colony.energy.allocate(cost);

        let dirs = if let Some(dir) = spawn.source_direction() {
            if let CreepRole::Excavator(source) = proto.role() && spawn.is_source_spawn(source) {
                vec![dir]
            } else {
                Direction::iter().filter(|dir2| **dir2 != dir).copied().collect_vec()
            }
        } else {
            Direction::iter().copied().collect_vec()
        };

        let name = self.scheduler.used_names.generate_new(proto.role());
        let result = spawn_id.resolve().spawn_creep_with_options(
            proto.body().parts(),
            &name,
            &SpawnOptions::new()
                .energy_structures(structures.into_iter().map_into::<Structure>())
                .directions(&dirs)
        );

        if let Err(err) = result {
            spawn.block();
            return Flow::Fail(err.into());
        }

        let id = game::creeps().get(name).expect("Creep should exist in Game.creeps right after spawning").id();
        spawn.begin_spawning(id.clone(), CreepData { role: proto.role().clone(), home: proto.home(), spawn_room: self.name.room_name() }, dirs);

        for colony in self.scheduler.colonies.values_mut() {
            colony.budget.add_creep(&proto);
        }

        self.scheduler.creeps.insert(id, proto);

        Flow::Continue(())
    }

    pub fn schedule_selected(&mut self, spawn_id: ObjectId<StructureSpawn>, body: Body, role: CreepRole) -> Flow {
        let home = self.name;
        self.schedule_selected_for(spawn_id, body, role, home)
    }

    pub fn schedule_for(&mut self, home: ColonyName, body: Body, role: CreepRole) -> Flow {
        let spawn_id = self.spawns().free()
            .max_by_key(|(_, spawn)| spawn.is_central())
            .map(|(id, _)| id)
            .or_break()?;

        self.schedule_selected_for(spawn_id, body, role, home)
    }

    pub fn schedule(&mut self, body: Body, role: CreepRole) -> Flow {
        let home = self.name;
        self.schedule_for(home, body, role)
    }
}
