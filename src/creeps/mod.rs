use std::fmt::Debug;

use log::warn;
use screeps::{Creep, ObjectId, RoomName, Source, StructureSpawn, find, game, look, prelude::*};
use serde::{Deserialize, Serialize};

use crate::{creeps::{dumptruck::DumptruckCreep, excavator::ExcavatorCreep, flagship::FlagshipCreep, remote_builder::RemoteBuilderCreep, tugboat::TugboatCreep, worker::WorkerCreep}, memory::Memory, statemachine::transition, utils::adjacent_positions};

mod flagship;
mod worker;
mod excavator;
mod remote_builder;
mod tugboat;
mod dumptruck;

#[derive(Serialize, Deserialize, Clone)]
pub struct CreepData {
    pub role: CreepRole,
    pub home: RoomName
}

impl CreepData {
    pub fn new(home: RoomName, role: CreepRole) -> Self {
        CreepData { role, home }
    }

    pub fn try_recover_from(creep: &Creep, mem: &Memory) -> Option<Self> {
        let home = mem.colony(creep.pos().room_name())
            .filter(|colony| colony.plan.center.spawn.is_complete())
            .or_else(|| 
                mem.colonies.values()
                .filter(|colony| colony.plan.center.spawn.is_complete())
                .min_by_key(|colony| colony.plan.center.pos.get_range_to(creep.pos()))
            )?;

        let role = match creep.name().split_ascii_whitespace().next()? {
            "Worker" => CreepRole::Worker(WorkerCreep::default()),
            "Flagship" => CreepRole::Flagship(FlagshipCreep::default()),
            "RemoteBuilder" => CreepRole::RemoteBuilder(RemoteBuilderCreep::default()),
            "Dumptruck" => CreepRole::Dumptruck(DumptruckCreep::default()),
            "Excavator" => {
                let source = adjacent_positions(creep.pos())
                    .flat_map(|pos| pos.look_for(look::SOURCES))
                    .flatten()
                    .next()
                    .or_else(|| creep.pos().find_closest_by_path(find::SOURCES, None))?;

                CreepRole::Excavator(ExcavatorCreep::default(), source.id()) 
            },
            _ => CreepRole::Scrap(get_recycle_spawn(creep, mem).id())
        };
        
        Some(CreepData::new(home.room_name, role))
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum CreepRole {
    Worker(WorkerCreep),
    Excavator(ExcavatorCreep, ObjectId<Source>),
    Flagship(FlagshipCreep),
    RemoteBuilder(RemoteBuilderCreep),
    Tugboat(TugboatCreep, ObjectId<Creep>),
    Scrap(ObjectId<StructureSpawn>),
    Dumptruck(DumptruckCreep)
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Hash, Clone)]
pub enum CreepType {
    Worker,
    Excavator(ObjectId<Source>), 
    Flagship,
    RemoteBuilder,
    Tugboat(ObjectId<Creep>),
    Scrap(ObjectId<StructureSpawn>),
    Dumptruck
}

impl CreepRole {
    pub fn get_type(&self) -> CreepType {
        match self {
            CreepRole::Worker(_) => CreepType::Worker,
            CreepRole::Flagship(_) => CreepType::Flagship,
            CreepRole::RemoteBuilder(_) => CreepType::RemoteBuilder,
            CreepRole::Excavator(_, source) => CreepType::Excavator(*source),
            CreepRole::Tugboat(_, tugged) => CreepType::Tugboat(*tugged),
            CreepRole::Scrap(source) => CreepType::Scrap(*source),
            CreepRole::Dumptruck(_) => CreepType::Dumptruck
        }
    }
}

impl CreepType {
    pub fn prefix(&self) -> &str {
        match self {
            CreepType::Worker => "Worker",
            CreepType::Flagship => "Flagship",
            CreepType::RemoteBuilder => "RemoteBuilder",
            CreepType::Excavator(_) => "Excavator",
            CreepType::Tugboat(_) => "Tugboat",
            CreepType::Scrap(_) => "Scrap",
            CreepType::Dumptruck => "Dumptruck"
        }
    }

    pub fn default_role(&self) -> CreepRole {
        match self {
            CreepType::Worker => CreepRole::Worker(WorkerCreep::default()),
            CreepType::Flagship => CreepRole::Flagship(FlagshipCreep::default()),
            CreepType::RemoteBuilder => CreepRole::RemoteBuilder(RemoteBuilderCreep::default()),
            CreepType::Excavator(source) => CreepRole::Excavator(ExcavatorCreep::default(), *source),
            CreepType::Tugboat(tugged) => CreepRole::Tugboat(TugboatCreep::default(), *tugged),
            CreepType::Scrap(spawn) => CreepRole::Scrap(*spawn),
            CreepType::Dumptruck => CreepRole::Dumptruck(DumptruckCreep::default())
        }
    }
}

fn do_recycle(creep: &Creep, mem: &mut Memory, spawn: &ObjectId<StructureSpawn>) -> ObjectId<StructureSpawn> {
    let Some(spawn) = spawn.resolve() else {
        warn!("Spawn for recycling did not resolve");
        return get_recycle_spawn(creep, mem).id();
    };

    if creep.pos().is_near_to(spawn.pos()) {
        spawn.recycle_creep(creep).ok();
    } else {
        mem.movement.smart_move_creep_to(creep, &spawn).ok();
    }

    spawn.id()
}

pub fn do_creeps(mem: &mut Memory) {
    use CreepRole::*;

    let updatable_creeps: Vec<_> = game::creeps().values()
        .filter(|creep| !creep.spawning())
        .filter(|creep| {
            if !mem.creeps.contains_key(&creep.name()) {
                let Some(config) = CreepData::try_recover_from(creep, mem) else {
                    warn!("Unable to recover creep data for {}", creep.name());
                    return false;
                };

                mem.creeps.insert(creep.name(), config);
            }

            true
        }).collect();

    let mut update_creeps = updatable_creeps.clone();
    while !update_creeps.is_empty() {
        for creep in &update_creeps {
            let role = mem.creep(creep).unwrap().role.clone();

            let new_role = match &role {
                Worker(state) => Worker(transition(state, creep, mem)),
                Flagship(state) => Flagship(transition(state, creep, mem)),
                RemoteBuilder(state) => RemoteBuilder(transition(state, creep, mem)),
                Excavator(state, source) => Excavator(transition(state, creep, mem), *source),
                Tugboat(state, tugged) => Tugboat(transition(state, creep, mem), *tugged),
                Scrap(spawn) => Scrap(do_recycle(creep, mem, spawn)),
                Dumptruck(state) => Dumptruck(transition(state, creep, mem))
            };

            mem.creeps.get_mut(&creep.name()).unwrap().role = new_role.clone();
        }

        for creep in &updatable_creeps {
            mem.messages.creep_quick(creep).flush();
        }

        update_creeps = updatable_creeps.iter()
            .filter(|creep| !mem.messages.creep_quick(creep).empty())
            .cloned()
            .collect();
    }

    for creep in &updatable_creeps {
        mem.messages.creep(creep).flush();
    }
}

fn get_recycle_spawn(creep: &Creep, mem: &Memory) -> StructureSpawn {
    if let Some(home_name) = mem.creep(creep).map(|creep_data| creep_data.home) {
        if creep.pos().room_name() == home_name {
            if let Some(spawn) = creep.pos().find_closest_by_path(find::MY_SPAWNS, None) {
                return spawn
            }
        }

        if let Some(home) = game::rooms().get(home_name) {
            if let Some(spawn) = home.find(find::MY_SPAWNS, None).into_iter().next() {
                return spawn
            }
        }
    }

    game::spawns().values()
        .min_by_key(|spawn| creep.pos().get_range_to(spawn.pos()))
        .unwrap()
}