use std::mem;

use log::*;
use screeps::{Creep, Room, RoomName, find, game, prelude::*};
use serde::{Deserialize, Serialize};

use crate::{creeps::{bootstrap::carrier::BootstrapCarrierState, claimer::{ClaimerState, get_claim_request}, harvester::HarvesterState}, memory::{Memory, SharedMemory}};

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
    Worker(HarvesterState), Claimer(ClaimerState), BootstrapCarrier(BootstrapCarrierState, RoomName)
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
        let memory = &mut memory.shared;
        match role {
            CreepRole::Worker(state) => state.transition(&(), &creep, memory),
            CreepRole::Claimer(state) => state.transition(&(), &creep, memory),
            CreepRole::BootstrapCarrier(state, data) => state.transition(data, &creep, memory),
        }
    }
}