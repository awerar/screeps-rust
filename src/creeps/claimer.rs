use screeps::{Creep, ObjectId, Position, StructureController, action_error_codes::ClaimControllerErrorCode, game, prelude::*};
use log::*;
use serde::{Deserialize, Serialize};

use crate::{memory::Memory, statemachine::StateMachine};

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Default, Clone)]
pub enum ClaimerCreep {
    #[default]
    Idle,
    GoingTo(Position),
    Claiming(Position, ObjectId<StructureController>)
}

impl StateMachine<Creep> for ClaimerCreep {
    fn update(&self, creep: &Creep, mem: &mut Memory) -> Result<Self, ()> {
        use ClaimerCreep::*;

        match &self {
            Idle => {
                if let Some(position) = mem.claim_requests.iter().next() {
                    Ok(GoingTo(*position))
                } else {
                    Ok(self.clone())
                }
            },
            GoingTo(target) => {
                if creep.pos().room_name() == target.room_name() {
                    if let Some(controller) = game::rooms().get(target.room_name()).and_then(|room| room.controller()) {
                        return Ok(Claiming(target.clone(), controller.id()))
                    }
                }

                mem.movement.smart_move_creep_to(creep, *target).ok();
                Ok(self.clone())
            }
            Claiming(request, controller) => {
                let controller = controller.resolve().ok_or(())?;

                if creep.pos().is_near_to(controller.pos()) {
                    match creep.claim_controller(&controller) {
                        Ok(()) => {
                            info!("Sucessfully claimed controller!");
                            mem.claim_requests.remove(request);

                            return Ok(Idle)
                        },
                        Err(ClaimControllerErrorCode::InvalidTarget) => {
                            creep.attack_controller(&controller).ok();
                        },
                        Err(_) => {
                            warn!("Unable to claim controller!");
                            mem.claim_requests.remove(request);

                            return Ok(Idle)
                        }
                    }
                } else {
                    mem.movement.smart_move_creep_to(creep, &controller).ok();
                }

                Ok(self.clone())
            },
        }
    }
}