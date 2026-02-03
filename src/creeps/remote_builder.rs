use screeps::{Creep, ObjectId, Position, ResourceType, RoomName, StructureController, game, prelude::*};
use log::*;
use serde::{Deserialize, Serialize};

use crate::{creeps::{CreepState, DatalessCreepState}, memory::SharedMemory};

#[derive(Serialize, Deserialize, Default, Debug, PartialEq, Eq)]
enum RemoteBuilderState {
    #[default]
    Idle,
    Refilling,
    Building(Position)
}

impl CreepState<RoomName> for RemoteBuilderState {
    fn execute(self, home: &RoomName, creep: &Creep, memory: &mut SharedMemory) -> Option<Self> {
        use RemoteBuilderState::*;

        match self {
            Idle => {
                if creep.store().get_used_capacity(Some(ResourceType::Energy)) == 0 {
                    return Refilling.execute(home, creep, memory);
                }

                if let Some(request) = memory.remote_build_requests.get_new_request() {
                    return Building(request).execute(home, creep, memory);
                }

                Some(Idle)
            },
            Refilling => todo!(),
            Building(position) => todo!(),
        }
    }
}