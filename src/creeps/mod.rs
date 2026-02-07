use std::fmt::Debug;

use log::*;
use screeps::{Creep, RoomName, game, prelude::*};
use serde::{Deserialize, Serialize};

use crate::{creeps::{claimer::ClaimerState, harvester::HarvesterState, remote_builder::RemoteBuilderState, worker::WorkerState}, memory::Memory};

mod claimer;
mod worker;
mod harvester;
mod remote_builder;

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
        let Some(role) = CreepRole::try_recover_from(creep) else { return None };

        let colony = mem.colony(creep.pos().room_name())
            .filter(|colony| colony.spawn().is_some())
            .or_else(|| 
                mem.colonies.values()
                .filter(|colony| colony.spawn().is_some())
                .min_by_key(|colony| colony.center.get_range_to(creep.pos()))
            )?;
        
        Some(CreepData::new(colony.room_name, role))
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum CreepRole {
    Worker(WorkerState),
    Harvester(HarvesterState),
    Claimer(ClaimerState),
    RemoteBuilder(RemoteBuilderState)
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub enum CreepType {
    Worker, Harvester, Claimer, RemoteBuilder
}

impl CreepRole {
    pub fn get_type(&self) -> CreepType {
        match self {
            CreepRole::Worker(_) => CreepType::Worker,
            CreepRole::Claimer(_) => CreepType::Claimer,
            CreepRole::RemoteBuilder(_) => CreepType::RemoteBuilder,
            CreepRole::Harvester(_) => CreepType::Harvester
        }
    }

    pub fn try_recover_from(creep: &Creep) -> Option<Self> {
        match creep.name().split_ascii_whitespace().next()? {
            "Worker" => Some(CreepRole::Worker(Default::default())),
            "Claimer" => Some(CreepRole::Claimer(Default::default())),
            "RemoteBuilder" => Some(CreepRole::RemoteBuilder(Default::default())),
            "Harvester" => Some(CreepRole::Harvester(Default::default())),
            _ => None
        }
    }
}

impl CreepType {
    pub fn prefix(&self) -> &str {
        match self {
            CreepType::Worker => "Worker",
            CreepType::Claimer => "Claimer",
            CreepType::RemoteBuilder => "RemoteBuilder",
            CreepType::Harvester => "Harvester",
        }
    }

    pub fn default_role(&self) -> CreepRole {
        match self {
            CreepType::Worker => CreepRole::Worker(Default::default()),
            CreepType::Claimer => CreepRole::Claimer(Default::default()),
            CreepType::RemoteBuilder => CreepRole::RemoteBuilder(Default::default()),
            CreepType::Harvester => CreepRole::Harvester(Default::default()),
        }
    }
}

pub fn do_creeps(mem: &mut Memory) {
    use CreepRole::*;

    for creep in game::creeps().values() {
        if !mem.creeps.contains_key(&creep.name()) {
            let Some(config) = CreepData::try_recover_from(&creep, mem) else {
                warn!("Unable to recover creep data for {}", creep.name());
                continue;
            };

            mem.creeps.insert(creep.name(), config);
        }

        let creep_data = mem.creeps[&creep.name()].clone();

        let new_role = match &creep_data.role {
            Worker(state) => Worker(transition(&state, &creep, mem)),
            Claimer(state) => Claimer(transition(&state, &creep, mem)),
            RemoteBuilder(state) => RemoteBuilder(transition(&state, &creep, mem)),
            Harvester(state) => Harvester(transition(&state, &creep, mem)),
        };

        mem.creeps.get_mut(&creep.name()).unwrap().role = new_role;
    }
}