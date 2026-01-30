use screeps::{Creep, Flag, game, prelude::*};
use log::*;
use serde::{Deserialize, Serialize};

use crate::movement::smart_move_creep_to;

#[derive(Serialize, Deserialize, Debug)]
pub enum ClaimerState {
    Idle,
    Claiming(String)
}

impl Default for ClaimerState {
    fn default() -> Self {
        ClaimerState::Idle
    }
}

pub fn get_claim_request() -> Option<Flag> {
    let flag = game::flags().values().filter(|flag| flag.name() == "Claim").next()?;

    if let Some(room) = flag.room() {
        if let Some(controller) = room.controller() {
            if controller.my() {
                return None
            }
        }
    }

    Some(flag)
}

pub fn do_claimer_creep(creep: &Creep, state: ClaimerState) -> Option<ClaimerState> {
    match &state {
        ClaimerState::Idle => {
            if let Some(flag) = get_claim_request() {
                Some(ClaimerState::Claiming(flag.name()))
            } else {
                Some(state)
            }
        },
        ClaimerState::Claiming(flag_name) => {
            let flag = game::flags().get(flag_name.clone())?;
            let controller = flag.room().and_then(|room| room.controller());

            if let Some(controller) = controller {
                smart_move_creep_to(creep, &controller).ok();
                if creep.pos().is_near_to(controller.pos()) {
                    if creep.claim_controller(&controller).is_ok() {
                        info!("Sucessfully claimed controller!");
                        flag.remove().ok();
                        return Some(ClaimerState::Idle)
                    }
                }
            } else {
                smart_move_creep_to(creep, &flag).ok();
            }

            Some(state)
        },
    }
}