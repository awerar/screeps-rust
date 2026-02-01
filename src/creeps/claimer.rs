use screeps::{Creep, Flag, game, prelude::*};
use log::*;
use serde::{Deserialize, Serialize};

use crate::{creeps::CreepState, memory::SharedMemory};

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

impl CreepState for ClaimerState {
    fn execute(self, creep: &Creep, memory: &mut SharedMemory) -> Option<Self> {
        match &self {
            ClaimerState::Idle => {
                if let Some(flag) = get_claim_request() {
                    Some(ClaimerState::Claiming(flag.name()))
                } else {
                    Some(self)
                }
            },
            ClaimerState::Claiming(flag_name) => {
                let flag = game::flags().get(flag_name.clone())?;
                let controller = flag.room().and_then(|room| room.controller());

                if let Some(controller) = controller {
                    memory.movement.smart_move_creep_to(creep, &controller).ok();
                    if creep.pos().is_near_to(controller.pos()) {
                        if creep.claim_controller(&controller).is_ok() {
                            info!("Sucessfully claimed controller!");
                            flag.remove().ok();
                            return Some(ClaimerState::Idle)
                        }
                    }
                } else {
                    memory.movement.smart_move_creep_to(creep, &flag).ok();
                }

                Some(self)
            },
        }
    }
}