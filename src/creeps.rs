use std::mem;

use log::*;
use screeps::{Room, find, game, prelude::*};
use serde::{Deserialize, Serialize};

use crate::{claimer::{ClaimerState, do_claimer_creep, get_claim_request}, harvester::{HarvesterState, do_harvester_creep}, memory::Memory};

#[derive(Serialize, Deserialize, Debug)]
pub enum Role {
    Worker(HarvesterState), Claimer(ClaimerState)
}

pub fn get_missing_roles(memory: &Memory, room: &Room) -> Vec<Role> {
    let mut result = Vec::new();

    let harvester_count = room.find(find::MY_CREEPS, None).into_iter()
        .filter(|creep| {
            if let Some(role) = memory.creeps.get(&creep.name()) {
                matches!(role, Role::Worker(_))
            } else { false }
        }).count();

    let missing_harvester_count = (memory.source_assignments.max_creeps() - harvester_count).max(0);
    result.extend((0..missing_harvester_count).map(|_| Role::Worker(HarvesterState::Idle)));

    if let Some(flag) = get_claim_request() {
        if memory.claimer_creep.is_none() {
            result.push(Role::Claimer(ClaimerState::Claiming(flag.name())));
        }
    }

    result
}

pub fn do_creeps(memory: &mut Memory) {
    for creep in game::creeps().values() {
        let role = memory.creeps.entry(creep.name()).or_insert_with(||
            Role::Worker(HarvesterState::Idle)
        );
        
        match role {
            Role::Worker(state) => {
                let new_state = do_harvester_creep(&creep, mem::take(state), &mut memory.source_assignments);
                if let Some(new_state) = new_state {
                    *state = new_state;
                } else {
                    warn!("Creep {} failed. Falling back to default state", creep.name());
                }
            },
            Role::Claimer(state) => {
                let new_state = do_claimer_creep(&creep, mem::take(state));
                if let Some(new_state) = new_state {
                    *state = new_state;
                } else {
                    warn!("Creep {} failed. Falling back to default state", creep.name());
                }
            },
        };
    }
}