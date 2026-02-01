use std::mem;

use log::*;
use screeps::{Creep, Room, find, game, prelude::*};
use serde::{Deserialize, Serialize};

use crate::{creeps::{claimer::{ClaimerState, get_claim_request}, harvester::HarvesterState}, memory::{Memory, SharedMemory}};

pub mod claimer;
pub mod harvester;

pub trait CreepData where {
    fn perform(&mut self, creep: &Creep, memory: &mut SharedMemory);
}

pub trait CreepState where Self : Sized, Self : Default {
    fn execute(self, creep: &Creep, memory: &mut SharedMemory) -> Option<Self>;
}

impl <T> CreepData for T where T : CreepState {
    fn perform(&mut self, creep: &Creep, memory: &mut SharedMemory) {
        let new_state = mem::take(self).execute(creep, memory);
        if let Some(new_state) = new_state {
            *self = new_state;
        } else {
            warn!("Creep {} failed. Falling back to default state", creep.name());
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub enum CreepRole {
    Worker(HarvesterState), Claimer(ClaimerState)
}

impl CreepData for CreepRole {
    fn perform(&mut self, creep: &Creep, memory: &mut SharedMemory) {
        match self {
            CreepRole::Worker(state) => state.perform(creep, memory),
            CreepRole::Claimer(state) => state.perform(creep, memory),
        }
    }
}

pub fn get_missing_roles(memory: &Memory, room: &Room) -> Vec<CreepRole> {
    let mut result = Vec::new();

    let harvester_count = room.find(find::MY_CREEPS, None).into_iter()
        .filter(|creep| {
            if let Some(role) = memory.creeps.get(&creep.name()) {
                matches!(role, CreepRole::Worker(_))
            } else { false }
        }).count();

        
    let missing_harvester_count = (memory.shared.source_assignments.max_creeps() - harvester_count).max(0);
    result.extend((0..missing_harvester_count).map(|_| CreepRole::Worker(HarvesterState::Idle)));

    if let Some(flag) = get_claim_request() {
        if memory.shared.claimer_creep.is_none() {
            result.push(CreepRole::Claimer(ClaimerState::Claiming(flag.name())));
        }
    }

    result
}

pub fn do_creeps(memory: &mut Memory) {
    for creep in game::creeps().values() {
        let role = memory.creeps.get_mut(&creep.name());
        match role {
            None => {
                error!("Creep {} has no role", creep.name())
            },
            Some(role) => role.perform(&creep, &mut memory.shared),
        }
    }
}