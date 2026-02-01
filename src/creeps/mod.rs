use std::mem;

use log::*;
use screeps::{Creep, Room, find, game, prelude::*};
use serde::{Deserialize, Serialize};

use crate::{creeps::{bootstrap::carrier::BootstrapCarrier, claimer::{ClaimerState, get_claim_request}, harvester::HarvesterState}, memory::{Memory, SharedMemory}};

pub mod claimer;
pub mod harvester;
pub mod bootstrap;

pub trait CreepData where {
    fn perform(&mut self, creep: &Creep, memory: &mut SharedMemory);
}

pub trait PureCreepState where Self : Sized + Default {
    fn execute(self, creep: &Creep, memory: &mut SharedMemory) -> Option<Self>;
}

impl <T> CreepData for T where T : PureCreepState {
    fn perform(&mut self, creep: &Creep, memory: &mut SharedMemory) {
        transition(self, creep, memory, Self::execute);
    }
}

fn transition<T, F>(state: &mut T, creep: &Creep, memory: &mut SharedMemory, f: F) 
where 
    T : Default,
    F : FnOnce(T, &Creep, &mut SharedMemory) -> Option<T>
{
    let new_state = f(mem::take(state), creep, memory);
    if let Some(new_state) = new_state {
        *state = new_state;
    } else {
        warn!("Creep {} failed. Falling back to default state", creep.name());
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub enum CreepRole {
    Worker(HarvesterState), Claimer(ClaimerState), BootstrapCarrier(BootstrapCarrier)
}

impl CreepData for CreepRole {
    fn perform(&mut self, creep: &Creep, memory: &mut SharedMemory) {
        match self {
            CreepRole::Worker(state) => state.perform(creep, memory),
            CreepRole::Claimer(state) => state.perform(creep, memory),
            CreepRole::BootstrapCarrier(carrier) => carrier.perform(creep, memory),
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
        let role = memory.creeps.entry(creep.name()).or_insert(CreepRole::Worker(HarvesterState::Idle));
        role.perform(&creep, &mut memory.shared);
    }
}