use std::fmt::Debug;

use log::*;
use screeps::{Creep, RoomName, game, prelude::*};
use serde::{Deserialize, Serialize};

use crate::{creeps::{claimer::ClaimerState, harvester::HarvesterState, remote_builder::RemoteBuilderState}, memory::Memory};

pub mod claimer;
pub mod harvester;
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

#[derive(Serialize, Deserialize)]
pub struct CreepConfig {
    pub home: RoomName
}

impl CreepConfig {
    fn try_construct_from(creep: &Creep, mem: &Memory) -> Option<Self> {
        let colony = mem.colony(creep.pos().room_name())
            .filter(|colony| colony.spawn().is_some())
            .or_else(|| 
                mem.colonies.values()
                .filter(|colony| colony.spawn().is_some())
                .min_by_key(|colony| colony.center.get_range_to(creep.pos()))
            )?;
        
        Some(CreepConfig { home: colony.room_name })
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum CreepRole {
    Worker(HarvesterState), 
    Claimer(ClaimerState),
    RemoteBuilder(RemoteBuilderState)
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub enum CreepType {
    Worker, Claimer, RemoteBuilder
}

impl CreepRole {
    pub fn get_type(&self) -> CreepType {
        match self {
            CreepRole::Worker(_) => CreepType::Worker,
            CreepRole::Claimer(_) => CreepType::Claimer,
            CreepRole::RemoteBuilder(_) => CreepType::RemoteBuilder,
        }
    }

    pub fn try_recover_from(creep: &Creep) -> Option<Self> {
        match creep.name().split_ascii_whitespace().next()? {
            "Worker" => Some(CreepRole::Worker(Default::default())),
            "Claimer" => Some(CreepRole::Claimer(Default::default())),
            "RemoteBuilder" => Some(CreepRole::RemoteBuilder(Default::default())),
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
        }
    }

    pub fn default_role(&self) -> CreepRole {
        match self {
            CreepType::Worker => CreepRole::Worker(Default::default()),
            CreepType::Claimer => CreepRole::Claimer(Default::default()),
            CreepType::RemoteBuilder => CreepRole::RemoteBuilder(Default::default()),
        }
    }
}

pub fn do_creeps(mem: &mut Memory) {
    use CreepRole::*;

    for creep in game::creeps().values() {
        if !mem.creeps.contains_key(&creep.name()) {
            if let Some(config) = CreepConfig::try_construct_from(&creep, mem) {
                mem.creeps.insert(creep.name(), config);
            } else {
                warn!("Unable to construct creep config for {}", creep.name());
            }
        }

        if !mem.machines.creeps.contains_key(&creep.name()) {
            let Some(role) = CreepRole::try_recover_from(&creep) else {
                warn!("Unable to recover role for {}", creep.name());
                continue; 
            };
            mem.machines.creeps.insert(creep.name(), role);
        }

        let role = mem.machines.creeps[&creep.name()].clone();
        let new_role = match &role {
            Worker(state) => Worker(transition(&state, &creep, mem)),
            Claimer(state) => Claimer(transition(&state, &creep, mem)),
            RemoteBuilder(state) => RemoteBuilder(transition(&state, &creep, mem))
        };

        *mem.machines.creeps.get_mut(&creep.name()).unwrap() = new_role;
    }
}