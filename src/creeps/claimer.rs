use screeps::{Creep, ObjectId, Position, StructureController, game, prelude::*};
use log::*;
use serde::{Deserialize, Serialize};

use crate::{creeps::DatalessCreepState, memory::SharedMemory};

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Default)]
pub enum ClaimerState {
    #[default]
    Idle,
    GoingTo(Position),
    Claiming(Position, ObjectId<StructureController>)
}

impl DatalessCreepState for ClaimerState {
    fn execute(self, creep: &Creep, memory: &mut SharedMemory) -> Option<Self> {
        match &self {
            ClaimerState::Idle => {
                if let Some(position) = memory.claim_requests.iter().next() {
                    ClaimerState::GoingTo(*position).execute(creep, memory)
                } else {
                    Some(self)
                }
            },
            ClaimerState::Claiming(request, controller) => {
                let controller = controller.resolve()?;

                if creep.pos().is_near_to(controller.pos()) {
                    if creep.claim_controller(&controller).is_ok() {
                        info!("Sucessfully claimed controller!");
                        memory.claim_requests.remove(request);

                        return Some(ClaimerState::Idle)
                    }
                } else {
                    memory.movement.smart_move_creep_to(creep, &controller).ok();
                }

                Some(self)
            },
            ClaimerState::GoingTo(target) => {
                if creep.pos().room_name() == target.room_name() {
                    if let Some(controller) = game::rooms().get(target.room_name()).and_then(|room| room.controller()) {
                        return ClaimerState::Claiming(target.clone(), controller.id()).execute(creep, memory)
                    }
                }

                memory.movement.smart_move_creep_to(creep, *target).ok();
                Some(self)
            }
        }
    }
}