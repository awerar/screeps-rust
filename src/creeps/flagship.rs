use enum_display::EnumDisplay;
use screeps::{Creep, Position, StructureController, action_error_codes::ClaimControllerErrorCode, game, prelude::*};
use log::{info, warn};
use serde::{Deserialize, Serialize};

use crate::{check::Check, ids::{CheckState, Checked, ById, Unchecked, WithId}, memory::ClaimRequests, movement::requests::MovementRequests, statemachine::Transition};

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Default, Clone, EnumDisplay)]
pub enum FlagshipCreep<I: CheckState = Checked> {
    #[default]
    Idle,
    GoingTo(Position),
    Claiming(Position, I::Repr<StructureController>)
}

impl<'de> Deserialize<'de> for FlagshipCreep {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let us = FlagshipCreep::<Unchecked>::deserialize(deserializer)?;
        Ok(match us {
            FlagshipCreep::Idle => Self::Idle,
            FlagshipCreep::GoingTo(pos) => Self::GoingTo(pos),
            FlagshipCreep::Claiming(pos, controller) => 
                controller.check().map(ById).map(|controller| Self::Claiming(pos, controller))
                    .unwrap_or(Self::GoingTo(pos)),
        })
    }
}

impl FlagshipCreep {
    pub fn update(self, creep: &WithId<Creep>, movement: &mut MovementRequests, claim_requests: &mut ClaimRequests) -> anyhow::Result<Transition<Self>> {
        use FlagshipCreep::*;
        use Transition::*;

        match &self {
            Idle => {
                if let Some(position) = claim_requests.iter().next() {
                    Ok(Continue(GoingTo(*position)))
                } else {
                    Ok(Break(self))
                }
            },
            GoingTo(target) => {
                if creep.pos().room_name() == target.room_name()
                    && let Some(controller) = game::rooms().get(target.room_name()).and_then(|room| room.controller()) {
                        return Ok(Continue(Claiming(*target, ById(controller))))
                    }

                let _ = movement.move_creep_to(creep, *target, 0);
                Ok(Break(self))
            }
            Claiming(request, controller) => {
                if movement.move_creep_to(creep, controller.pos(), 1).in_range() {
                    match creep.claim_controller(controller) {
                        Ok(()) => {
                            info!("Sucessfully claimed controller!");
                            claim_requests.remove(request);

                            return Ok(Continue(Idle))
                        },
                        Err(ClaimControllerErrorCode::InvalidTarget) => {
                            creep.attack_controller(controller)?;
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