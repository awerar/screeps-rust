use std::{collections::HashMap, fmt::Debug};

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
            return transition(&S::default(), creep, mem)
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
    home: RoomName
}

impl CreepConfig {
    fn try_construct_from(creep: &Creep, mem: &Memory) -> Option<Self> {
        let colony = mem.colony(creep.pos().room_name())?;

        Some(CreepConfig { home: colony.room_name })
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum CreepRole {
    Worker(HarvesterState), 
    Claimer(ClaimerState),
    RemoteBuilder(RemoteBuilderState)
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Hash)]
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

    pub fn prefix(&self) -> &str {
        match self {
            CreepRole::Worker(_) => "Worker",
            CreepRole::Claimer(_) => "Claimer",
            CreepRole::RemoteBuilder(_) => "RemoteBuilder",
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

fn get_current_roles(mem: &Memory) -> HashMap<CreepType, usize> {
    let mut result = HashMap::new();
    for role_type in mem.machines.creeps.values().map(|role| role.get_type()) {
        *result.entry(role_type).or_default() += 1;
    }

    result
}

pub fn get_missing_roles_in(mem: &Memory, colony_name: RoomName) -> Vec<CreepRole> {
    let mut result = Vec::new();

    let current_roles = get_current_roles(mem);
        
    let current_harvesters = current_roles.get(&CreepType::Worker).unwrap_or(&0);
    let target_harvesters = mem.source_assignments.get(&colony_name).map(|x| x.max_creeps()).unwrap_or(0);
    let missing_harvester_count = (target_harvesters - current_harvesters).max(0);
    result.extend((0..missing_harvester_count).map(|_| CreepRole::Worker(Default::default())));

    let any_claimers = *current_roles.get(&CreepType::Claimer).unwrap_or(&0) > 0;
    if mem.claim_requests.len() > 0 && !any_claimers {
        result.push(CreepRole::Claimer(Default::default()));
    }

    let current_remote_builders = current_roles.get(&CreepType::RemoteBuilder).unwrap_or(&0);
    let target_remote_builders = mem.remote_build_requests.get_total_work_ticks().div_ceil(750) as usize;
    let missing_remote_builder_count = target_remote_builders - current_remote_builders;
    result.extend((0..missing_remote_builder_count).map(|_| CreepRole::RemoteBuilder(Default::default())));

    result
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