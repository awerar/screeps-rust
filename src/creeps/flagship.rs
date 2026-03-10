use enum_display::EnumDisplay;
use screeps::{Creep, Position, StructureController, action_error_codes::ClaimControllerErrorCode, game, prelude::*};
use log::{info, warn};
use serde::{Deserialize, Serialize};

use crate::{memory::ClaimRequests, movement::MovementSolver, safeid::{GetSafeID, IDKind, SafeID, SafeIDs, TryMakeSafe, UnsafeIDs}, statemachine::{StateMachine, Transition}};

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Default, Clone, EnumDisplay)]
pub enum FlagshipCreep<I: IDKind = SafeIDs> {
    #[default]
    Idle,
    GoingTo(Position),
    Claiming(Position, I::ID<StructureController>)
}

impl<'de> Deserialize<'de> for FlagshipCreep {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let us = FlagshipCreep::<UnsafeIDs>::deserialize(deserializer)?;
        Ok(match us {
            FlagshipCreep::Idle => Self::Idle,
            FlagshipCreep::GoingTo(pos) => Self::GoingTo(pos),
            FlagshipCreep::Claiming(pos, controller) => 
                controller.try_make_safe().map(|controller| Self::Claiming(pos, controller))
                    .unwrap_or(Self::GoingTo(pos)),
        })
    }
}

type Args<'a> = (&'a mut MovementSolver, &'a mut ClaimRequests);
impl StateMachine<SafeID<Creep>, Args<'_>> for FlagshipCreep {
    fn update(self, creep: &SafeID<Creep>, args: &mut Args<'_>) -> anyhow::Result<Transition<Self>> {
        use FlagshipCreep::*;
        use Transition::*;

        let (movement_solver, claim_requests) = args;

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
                        return Ok(Continue(Claiming(*target, controller.safe_id())))
                    }
                }

                movement_solver.move_creep_to(creep, *target, 0);
                Ok(Break(self))
            }
            Claiming(request, controller) => {
                if movement_solver.move_creep_to(creep, controller.pos(), 1) {
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
                }

                Ok(Break(self))
            },
        }
    }
}