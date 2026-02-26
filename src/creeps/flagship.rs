use enum_display::EnumDisplay;
use anyhow::anyhow;
use screeps::{Creep, ObjectId, Position, StructureController, action_error_codes::ClaimControllerErrorCode, game, prelude::*};
use log::{info, warn};
use serde::{Deserialize, Serialize};

use crate::{id::Resolved, memory::ClaimRequests, movement::Movement, statemachine::{StateMachine, Transition}};

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Default, Clone, EnumDisplay)]
pub enum FlagshipCreep {
    #[default]
    Idle,
    GoingTo(Position),
    Claiming(Position, ObjectId<StructureController>)
}

pub type Args<'a> = (&'a mut Movement<Resolved>, &'a mut ClaimRequests);
impl StateMachine<Creep, Args<'_>> for FlagshipCreep {
    fn update(self, creep: &Creep, args: &mut Args<'_>) -> anyhow::Result<Transition<Self>> {
        use FlagshipCreep::*;
        use Transition::*;

        let (movement, claim_requests) = args;

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
                let controller = controller.resolve().ok_or(anyhow!("Unable to resolve controller"))?;

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