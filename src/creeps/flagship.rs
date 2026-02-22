use std::fmt::Display;

use screeps::{Creep, ObjectId, Position, StructureController, action_error_codes::ClaimControllerErrorCode, game, prelude::*};
use log::{info, warn};
use serde::{Deserialize, Serialize};

use crate::{memory::ClaimRequests, movement::Movement, statemachine::{StateMachine, Transition}};

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Default, Clone)]
pub enum FlagshipCreep {
    #[default]
    Idle,
    GoingTo(Position),
    Claiming(Position, ObjectId<StructureController>)
}

impl Display for FlagshipCreep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        todo!()
    }
}

type Data = ();
type Systems = (Movement, ClaimRequests);
impl StateMachine<Creep, Data, Systems> for FlagshipCreep {
    fn update(self, creep: &Creep, _: &Data, systems: &mut Systems) -> Result<Transition<Self>, ()> {
        use FlagshipCreep::*;
        use Transition::*;

        let (movement, claim_requests) = systems;

        match &self {
            Idle => {
                if let Some(position) = claim_requests.iter().next() {
                    Ok(Continue(GoingTo(*position)))
                } else {
                    Ok(Break(self))
                }
            },
            GoingTo(target) => {
                if creep.pos().room_name() == target.room_name() {
                    if let Some(controller) = game::rooms().get(target.room_name()).and_then(|room| room.controller()) {
                        return Ok(Continue(Claiming(*target, controller.id())))
                    }
                }

                movement.smart_move_creep_to(creep, *target).ok();
                Ok(Break(self))
            }
            Claiming(request, controller) => {
                let controller = controller.resolve().ok_or(())?;

                if creep.pos().is_near_to(controller.pos()) {
                    match creep.claim_controller(&controller) {
                        Ok(()) => {
                            info!("Sucessfully claimed controller!");
                            claim_requests.remove(request);

                            return Ok(Continue(Idle))
                        },
                        Err(ClaimControllerErrorCode::InvalidTarget) => {
                            creep.attack_controller(&controller).ok();
                        },
                        Err(_) => {
                            warn!("Unable to claim controller!");
                            claim_requests.remove(request);

                            return Ok(Continue(Idle))
                        }
                    }
                } else {
                    movement.smart_move_creep_to(creep, &controller).ok();
                }

                Ok(Break(self))
            },
        }
    }
}