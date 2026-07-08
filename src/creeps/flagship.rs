use std::collections::HashSet;

use anyhow::anyhow;
use enum_display::EnumDisplay;
use itertools::Itertools;
use log::warn;
use screeps::{HasPosition, OwnedStructureProperties, Position, RoomName, game};
use serde::{Deserialize, Serialize};

use crate::{commands::{Command, handle_commands}, creeps::virtual_creep::VirtualCreep, defer, defer_err, done_if, movement::requests::MovementRequests, next, next_if, statemachine::Transition};

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Default, Clone, EnumDisplay)]
pub enum FlagshipCreep {
    #[default]
    Idle,
    Claiming { room: RoomName, controller_pos: Option<Position> }
}

#[derive(Serialize, Deserialize, Default)]
pub struct FlagshipCoordinator {
    pub rooms: HashSet<RoomName>,
}

impl FlagshipCoordinator {
    pub fn update(&mut self) {
        for room in self.rooms.iter().copied().collect_vec() {
            if game::rooms().get(room).is_some_and(|room| room.controller().is_none_or(|controller| controller.my())) {
                self.rooms.remove(&room);
            }
        }

        handle_commands(|cmd| {
            if let Command::Claim { room } = cmd {
                let name = match RoomName::new(room) {
                    Ok(name) => name,
                    Err(err) => { warn!("Invalid room name: {err}"); return true },
                };

                self.rooms.insert(name);
                true
            } else { false }
        });
    }
}

impl FlagshipCreep {
    pub fn update(mut self, creep: &mut VirtualCreep, movement: &mut MovementRequests, coordinator: &mut FlagshipCoordinator) -> anyhow::Result<Transition<Self>> {
        use FlagshipCreep::*;
        use Transition::*;

        match &mut self {
            Idle => {
                if let Some(room) = coordinator.rooms.iter().next().copied() {
                    next!(Claiming { room, controller_pos: None });
                }

                Ok(Done(Idle))
            },
            Claiming { room: room_name, controller_pos } => {
                next_if!(!coordinator.rooms.contains(room_name), Idle);

                let room = game::rooms().get(*room_name);
                let controller = room
                    .map(|room| {
                        room.controller().ok_or_else(|| {
                            coordinator.rooms.remove(room_name);
                            anyhow!("{room_name} does not have a controller! Removing its claim request")
                        })
                    }).transpose()?;

                if controller_pos.is_none() && let Some(controller) = &controller {
                    *controller_pos = Some(controller.pos());
                }

                let target = controller_pos.unwrap_or_else(|| Position::new(25.try_into().unwrap(), 25.try_into().unwrap(), *room_name));
                defer!(movement.move_vcreep_to(creep, target, 1), self)?;

                let controller = controller.expect("Creep should be in the room and the room should have a controller");
                defer_err!(creep.claim_controller(controller.clone()), self)?;

                done_if!(controller.reservation().is_some_and(|r| r.ticks_to_end() > 0), self);

                coordinator.rooms.remove(room_name);
                Ok(Next(Idle))
            }
        }
    }
}