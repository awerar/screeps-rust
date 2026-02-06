use screeps::{Creep, Position, ResourceType, prelude::*};
use serde::{Deserialize, Serialize};

use crate::{creeps::CreepState, memory::Memory};

#[derive(Serialize, Deserialize, Default, Debug, PartialEq, Eq, Clone)]
pub enum RemoteBuilderState {
    #[default]
    Idle,
    Refilling,
    Building(Position)
}

impl CreepState for RemoteBuilderState {
    fn update(&self, creep: &Creep, mem: &mut Memory) -> Result<Self, ()> {
        use RemoteBuilderState::*;

        match self {
            Idle => {
                if creep.store().get_used_capacity(Some(ResourceType::Energy)) == 0 {
                    return Ok(Refilling);
                }

                if let Some(request) = mem.remote_build_requests.get_new_request() {
                    return Ok(Building(request));
                }

                Ok(self.clone())
            },
            Refilling => {
                let colony = mem.colony(mem.creep(creep).ok_or(())?.home).ok_or(())?;

                if !creep.pos().is_near_to(colony.buffer_pos) {
                    mem.movement.smart_move_creep_to(creep, colony.buffer_pos).ok();
                    return Ok(self.clone())
                }

                let buffer = colony.buffer_structure().ok_or(())?;
                creep.withdraw(buffer.as_withdrawable().ok_or(())?, ResourceType::Energy, None).map_err(|_| ())?;
                
                if let Some(request) = mem.remote_build_requests.get_new_request() {
                    Ok(Building(request))
                } else {
                    Ok(Idle)
                }
            },
            Building(pos) => {
                let Some(build_data) = mem.remote_build_requests.get_request_data(pos) else {
                    return Ok(Idle)
                };

                if creep.store().get_used_capacity(Some(ResourceType::Energy)) == 0 {
                    return Ok(Refilling)
                }

                if creep.pos().in_range_to(build_data.pos, 3) {
                    let Some(site) = build_data.site() else {
                        return Ok(Idle)
                    };

                    creep.build(&site).map_err(|_| ())?
                }
                
                if !creep.pos().is_near_to(build_data.pos) {
                    mem.movement.smart_move_creep_to(creep, build_data.pos).ok();
                }

                Ok(self.clone())
            },
        }
    }
}