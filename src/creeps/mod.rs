use std::fmt::Debug;

use log::*;
use screeps::{Creep, ObjectId, RoomName, Source, StructureSpawn, find, game, look, prelude::*};
use serde::{Deserialize, Serialize};

use crate::{creeps::{claimer::ClaimerState, harvester::HarvesterState, remote_builder::RemoteBuilderState, tugboat::TugboatState, worker::WorkerState}, memory::Memory, utils::adjacent_positions};

mod claimer;
mod worker;
mod harvester;
mod remote_builder;
mod tugboat;

pub trait CreepState where Self : Sized + Default + Eq + Debug {
    fn update(&self, creep: &Creep, mem: &mut Memory) -> Result<Self, ()>;
}

fn transition<S>(state: &S, creep: &Creep, mem: &mut Memory) -> S where S : CreepState {
    let Ok(new_state) = state.update(creep, mem) else {
        if *state == S::default() {
            error!("{} failed on default state", creep.name());
            return S::default()
        } else {
            error!("{} failed on state {:?}. Falling back to default state", creep.name(), state);
            return S::default() // TODO: This should probably execute the default state
        }
    };

    if new_state != *state {
        transition(&new_state, creep, mem)
    } else {
        new_state
    }
}


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
            .filter(|colony| colony.spawn().is_some())
            .or_else(|| 
                mem.colonies.values()
                .filter(|colony| colony.spawn().is_some())
                .min_by_key(|colony| colony.center.get_range_to(creep.pos()))
            )?;

        let role = match creep.name().split_ascii_whitespace().next()? {
            "Worker" => CreepRole::Worker(Default::default()),
            "Claimer" => CreepRole::Claimer(Default::default()),
            "RemoteBuilder" => CreepRole::RemoteBuilder(Default::default()),
            "Harvester" => {
                let source = adjacent_positions(creep.pos())
                    .flat_map(|pos| pos.look_for(look::SOURCES))
                    .flatten()
                    .next()
                    .or_else(|| creep.pos().find_closest_by_path(find::SOURCES, None))?;

                CreepRole::Harvester(Default::default(), source.id()) 
            },
            _ => CreepRole::Recycle(get_recycle_spawn(creep, mem).id())
        };
        
        Some(CreepData::new(home.room_name, role))
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum CreepRole {
    Worker(WorkerState),
    Harvester(HarvesterState, ObjectId<Source>),
    Claimer(ClaimerState),
    RemoteBuilder(RemoteBuilderState),
    Tugboat(TugboatState, ObjectId<Creep>),
    Recycle(ObjectId<StructureSpawn>)
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Hash, Clone)]
pub enum CreepType {
    Worker,
    Harvester(ObjectId<Source>), 
    Claimer,
    RemoteBuilder,
    Tugboat(ObjectId<Creep>),
    Recycle(ObjectId<StructureSpawn>)
}

impl CreepRole {
    pub fn get_type(&self) -> CreepType {
        match self {
            CreepRole::Worker(_) => CreepType::Worker,
            CreepRole::Claimer(_) => CreepType::Claimer,
            CreepRole::RemoteBuilder(_) => CreepType::RemoteBuilder,
            CreepRole::Harvester(_, source) => CreepType::Harvester(*source),
            CreepRole::Tugboat(_, tugged) => CreepType::Tugboat(*tugged),
            CreepRole::Recycle(source) => CreepType::Recycle(*source)
        }
    }
}

impl CreepType {
    pub fn prefix(&self) -> &str {
        match self {
            CreepType::Worker => "Worker",
            CreepType::Claimer => "Claimer",
            CreepType::RemoteBuilder => "RemoteBuilder",
            CreepType::Harvester(_) => "Harvester",
            CreepType::Tugboat(_) => "Tugboat",
            CreepType::Recycle(_) => "Recycle"
        }
    }

    pub fn default_role(&self) -> CreepRole {
        match self {
            CreepType::Worker => CreepRole::Worker(Default::default()),
            CreepType::Claimer => CreepRole::Claimer(Default::default()),
            CreepType::RemoteBuilder => CreepRole::RemoteBuilder(Default::default()),
            CreepType::Harvester(source) => CreepRole::Harvester(Default::default(), *source),
            CreepType::Tugboat(tugged) => CreepRole::Tugboat(Default::default(), *tugged),
            CreepType::Recycle(spawn) => CreepRole::Recycle(*spawn)
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
                let Some(config) = CreepData::try_recover_from(&creep, mem) else {
                    warn!("Unable to recover creep data for {}", creep.name());
                    return false;
                };

                mem.creeps.insert(creep.name(), config);
            }

            true
        }).collect();

    let mut update_creeps = updatable_creeps.clone();
    while update_creeps.len() > 0 {
        for creep in &update_creeps {
            let role = mem.creep(creep).unwrap().role.clone();

            let new_role = match &role {
                Worker(state) => Worker(transition(&state, creep, mem)),
                Claimer(state) => Claimer(transition(&state, creep, mem)),
                RemoteBuilder(state) => RemoteBuilder(transition(&state, creep, mem)),
                Harvester(state, source) => Harvester(transition(&state, creep, mem), *source),
                Tugboat(state, tugged) => Tugboat(transition(&state, &creep, mem), *tugged),
                Recycle(spawn) => Recycle(do_recycle(creep, mem, spawn)),
            };

            mem.creeps.get_mut(&creep.name()).unwrap().role = new_role.clone();
        }

        for creep in &updatable_creeps {
            mem.messages.creep_quick(&creep).flush();
        }

        update_creeps = updatable_creeps.iter()
            .filter(|creep| !mem.messages.creep_quick(creep).empty())
            .cloned()
            .collect()
    }

    for creep in &updatable_creeps {
        mem.messages.creep(&creep).flush();
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