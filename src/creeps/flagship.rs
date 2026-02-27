use enum_display::EnumDisplay;
use screeps::{Creep, Position, StructureController, action_error_codes::ClaimControllerErrorCode, game, prelude::*};
use log::{info, warn};
use serde::{Deserialize, Serialize};

use crate::{id::{IDMaybeResolvable, IDMode, IDResolvable, IntoResolvedID, Resolved, Unresolved}, memory::ClaimRequests, movement::Movement, statemachine::{StateMachine, Transition}};

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Default, Clone, EnumDisplay)]
pub enum FlagshipCreep<M: IDMode> {
    #[default]
    Idle,
    GoingTo(Position),
    Claiming(Position, M::Wrap<StructureController>)
}

impl IDResolvable for FlagshipCreep<Unresolved> {
    type Target = FlagshipCreep<Resolved>;

    fn id_resolve(self) -> Self::Target {
        match self {
            Self::Idle => FlagshipCreep::Idle,
            Self::GoingTo(position) => FlagshipCreep::GoingTo(position),
            Self::Claiming(position, controller) => 
                controller.try_id_resolve().map(|controller| FlagshipCreep::Claiming(position, controller))
                    .unwrap_or(FlagshipCreep::Idle),
        }
    }
}

pub type Args<'a> = (&'a mut Movement<Resolved>, &'a mut ClaimRequests);
impl StateMachine<Creep, Args<'_>> for FlagshipCreep<Resolved> {
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
                        return Ok(Continue(Claiming(*target, controller.into_rid())))
                    }
                }

                movement.smart_move_creep_to(creep, *target).ok();
                Ok(Break(self))
            }
            Claiming(request, controller) => {
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
                    movement.smart_move_creep_to(creep, controller.pos()).ok();
                }

                Ok(Break(self))
            },
        }
    }
}