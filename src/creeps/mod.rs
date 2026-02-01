use std::{collections::HashMap, mem};

use log::*;
use screeps::{Creep, RoomName, game, prelude::*};
use serde::{Deserialize, Serialize};

use crate::{creeps::{bootstrap::carrier::BootstrapCarrierState, claimer::ClaimerState, harvester::HarvesterState}, memory::{Memory, SharedMemory}};

pub mod claimer;
pub mod harvester;
pub mod bootstrap;

pub trait CreepState<D> where Self : Sized + Default {
    fn execute(self, data: &D, creep: &Creep, memory: &mut SharedMemory) -> Option<Self>;

    fn transition(&mut self, data: &D, creep: &Creep, memory: &mut SharedMemory) {
        let new_state = mem::take(self).execute(data, creep, memory);
        if let Some(new_state) = new_state {
            *self = new_state;
        } else {
            warn!("Creep {} failed. Falling back to default state", creep.name());
        }
    }
}

pub trait DatalessCreepState where Self : Sized + Default {
    fn execute(self, creep: &Creep, memory: &mut SharedMemory) -> Option<Self>;
}

impl<T> CreepState<()> for T where T : DatalessCreepState {
    fn execute(self, _: &(), creep: &Creep, memory: &mut SharedMemory) -> Option<Self> {
        self.execute(creep, memory)
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub enum CreepRole {
    Worker(HarvesterState), 
    Claimer(ClaimerState), 
    BootstrapCarrier(BootstrapCarrierState, RoomName)
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Hash)]
pub enum CreepType {
    Worker, Claimer, BootstrapCarrier
}

impl CreepRole {
    pub fn get_type(&self) -> CreepType {
        match self {
            CreepRole::Worker(_) => CreepType::Worker,
            CreepRole::Claimer(_) => CreepType::Claimer,
            CreepRole::BootstrapCarrier(_, _) => CreepType::BootstrapCarrier,
        }
    }

    pub fn prefix(&self) -> &str {
        match self {
            CreepRole::Worker(_) => "Worker",
            CreepRole::Claimer(_) => "Claimer",
            CreepRole::BootstrapCarrier(_, _) => "BootstrapCarrier",
        }
    }

    pub fn try_recover_from(creep: &Creep) -> Option<Self> {
        match creep.name().split_ascii_whitespace().next()? {
            "Worker" => Some(CreepRole::Worker(Default::default())),
            "Claimer" => Some(CreepRole::Claimer(Default::default())),
            _ => None
        }
    }
}

fn get_current_roles(memory: &Memory) -> HashMap<CreepType, usize> {
    let mut result = HashMap::new();
    for role_type in memory.creeps.values().map(|role| role.get_type()) {
        *result.entry(role_type).or_default() += 1;
    }

    result
}

pub fn get_missing_roles(memory: &Memory) -> Vec<CreepRole> {
    let mut result = Vec::new();

    let current_roles = get_current_roles(memory);
        
    let current_harvesters = current_roles.get(&CreepType::Worker).unwrap_or(&0);
    let missing_harvester_count = (memory.shared.source_assignments.max_creeps() - current_harvesters).max(0);
    result.extend((0..missing_harvester_count).map(|_| CreepRole::Worker(HarvesterState::Idle)));

    let any_claimers = *current_roles.get(&CreepType::Claimer).unwrap_or(&0) > 0;
    if memory.shared.claim_requests.len() > 0 && !any_claimers {
        result.push(CreepRole::Claimer(Default::default()));
    }

    result
}

pub fn do_creeps(memory: &mut Memory) {
    for creep in game::creeps().values() {
        let role = memory.creeps.get_mut(&creep.name());
        let role = match role {
            Some(role) => role,
            None => {
                let new_role = CreepRole::try_recover_from(&creep);
                let Some(new_role) = new_role else {
                    error!("Unable to recover role of {}", creep.name());
                    continue;
                };

                memory.creeps.try_insert(creep.name(), new_role).unwrap()
            },
        };

        let memory = &mut memory.shared;
        match role {
            CreepRole::Worker(state) => state.transition(&(), &creep, memory),
            CreepRole::Claimer(state) => state.transition(&(), &creep, memory),
            CreepRole::BootstrapCarrier(state, data) => state.transition(data, &creep, memory),
        }
    }
}