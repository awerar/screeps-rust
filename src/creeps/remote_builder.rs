use screeps::{Creep, Position, ResourceType, prelude::*};
use serde::{Deserialize, Serialize};

use crate::{memory::Memory, statemachine::{StateMachine, Transition}};

#[derive(Serialize, Deserialize, Default, Debug, PartialEq, Eq, Clone)]
pub enum RemoteBuilderCreep {
    #[default]
    Idle,
    Refilling,
    Building(Position)
}

impl StateMachine<Creep> for RemoteBuilderCreep {
    fn update(&self, creep: &Creep, mem: &mut Memory) -> Result<Transition<Self>, ()> {
        use RemoteBuilderCreep::*;
        use Transition::*;

        match self {
            Idle => {
                if creep.store().get_used_capacity(Some(ResourceType::Energy)) == 0 {
                    return Ok(Continue(Refilling));
                }

                if let Some(request) = mem.remote_build_requests.get_new_request() {
                    return Ok(Continue(Building(request)));
                }

                Ok(Stay)
            },
            Refilling => {
                let colony = mem.colony(mem.creep(creep).ok_or(())?.home).ok_or(())?;
                let buffer = colony.buffer().ok_or(())?;

                if !creep.pos().is_near_to(buffer.pos()) {
                    mem.movement.smart_move_creep_to(creep, buffer).ok();
                    return Ok(Stay)
                }

                creep.withdraw(buffer.withdrawable(), ResourceType::Energy, None).map_err(|_| ())?;
                
                if let Some(request) = mem.remote_build_requests.get_new_request() {
                    Ok(Continue(Building(request)))
                } else {
                    Ok(Break(Idle))
                }
            },
            Building(pos) => {
                let Some(build_data) = mem.remote_build_requests.get_request_data(*pos) else {
                    return Ok(Continue(Idle))
                };

                if creep.store().get_used_capacity(Some(ResourceType::Energy)) == 0 {
                    return Ok(Continue(Refilling))
                }

                if creep.pos().in_range_to(build_data.pos, 3) {
                    let Some(site) = build_data.site() else {
                        return Ok(Continue(Idle))
                    };

                    creep.build(&site).map_err(|_| ())?;
                }
                
                if !creep.pos().is_near_to(build_data.pos) {
                    mem.movement.smart_move_creep_to(creep, build_data.pos).ok();
                }

                Ok(Stay)
            },
        }
    }
}