use std::{collections::HashSet, mem};

use screeps::{Flag, HasPosition, OwnedStructureProperties, Position, Room, RoomName, StructureContainer, StructureObject, StructureSpawn, StructureType, find, game};
use serde::{Deserialize, Serialize};
use log::*;

use crate::{memory::{Memory, SharedMemory}, planning::plan_center_structures_in};

const CLAIM_FLAG_PREFIX: &str = "Claim";

#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum RoomStage {
    Claim,
    BuildBuffer,
    BuildSpawn,
    UpgradeBuffer,
    UpgradeController
}

impl Default for RoomStage {
    fn default() -> Self {
        RoomStage::Claim
    }
}

#[derive(Serialize, Deserialize)]
pub struct RoomData {
    pub name: RoomName,
    pub stage: RoomStage,
    pub center: Position
}

impl RoomData {
    pub fn room(&self) -> Option<Room> {
        game::rooms().get(self.name)
    }

    fn from(room_name: RoomName) -> Option<Self> {
        let center = find_claim_flags().into_iter()
            .map(|flag| flag.pos())
            .find(|pos| pos.room_name() == room_name)
            .or_else(|| {
                game::rooms().get(room_name).and_then(|room| {
                    let structures = room.find(find::MY_STRUCTURES, None);
                    structures.iter()
                        .find(|structure| structure.structure_type() == StructureType::Spawn)
                        .map(|spawn| spawn.pos())
                })
            })?;

        Some(RoomData {
            name: room_name,
            stage: Default::default(),
            center
        })
    }

    fn get_container_buffer(&self) -> Option<StructureContainer> {
        let structures = self.room()?.find(find::MY_STRUCTURES, None);

        structures.iter().find(|structure| {
                structure.structure_type() == StructureType::Container &&
                structure.pos().get_range_to(self.center) == 1
            }).cloned()
            .map(|structure| {
                let StructureObject::StructureContainer(container) = structure else { unreachable!() };
                container
            })
    }

    fn find_current_stage(&self) -> RoomStage {
        let Some(room) = game::rooms().get(self.name) else { return RoomStage::Claim };
        let controller = room.controller().unwrap();

        if controller.owner().is_none() { return RoomStage::Claim };
        
        let structures = room.find(find::MY_STRUCTURES, None);
        
        let buffer = room.storage()
            .map(|storage| StructureObject::StructureStorage(storage))
            .or_else(|| self.get_container_buffer().map(|container| container.into()));
        
        let Some(buffer) = buffer else { return RoomStage::BuildBuffer };

        let spawn = structures.iter()
            .flat_map(|structure| {
                if let StructureObject::StructureSpawn(spawn) = structure { Some(spawn) }
                else { None }
            }).next().cloned();

        let Some(_) = spawn else { return RoomStage::BuildSpawn; };
        if buffer.structure_type() == StructureType::Container { return RoomStage::UpgradeBuffer; }

        RoomStage::UpgradeController
    }

    fn update(&mut self, memory: &mut SharedMemory) -> Option<()> {
        match &self.stage {
            RoomStage::Claim => {
                if memory.claim_requests.insert(self.center) {
                    info!("Requesting new claim for room {}", self.name);
                }
            },
            RoomStage::BuildBuffer => {
                let already_planned = self.room()?.find(find::MY_CONSTRUCTION_SITES, None).into_iter()
                    .any(|structure| {
                        structure.structure_type() == StructureType::Container &&
                        structure.pos().get_range_to(self.center) == 1
                    });

                if !already_planned {
                    info!("Planning buffer in {}", self.name);
                    plan_center_structures_in(self, vec![StructureType::Container]);
                }

            },
            RoomStage::BuildSpawn => {
                
            },
            RoomStage::UpgradeBuffer => {
                
            },
            RoomStage::UpgradeController => {
                
            },
        }

        Some(())
    }
}

fn find_claim_flags() -> Vec<Flag> {
    game::flags().entries()
        .filter(|(name, _)| name.starts_with(CLAIM_FLAG_PREFIX))
        .map(|(_, flag)| flag)
        .collect()
}

pub fn update_rooms(memory: &mut Memory) {
    info!("Updating rooms...");

    let owned_rooms = game::rooms().entries()
        .filter(|(_, room)| {
            if let Some(controller) = room.controller() { controller.my() }
            else { false }
        }).map(|(name, _)| name);


    let claim_rooms = find_claim_flags().into_iter()
        .map(|flag| flag.pos().room_name());

    let curr_rooms: HashSet<_> = owned_rooms.chain(claim_rooms).collect();
    let prev_rooms: HashSet<_> = memory.rooms.keys().cloned().collect();

    let lost_rooms = prev_rooms.difference(&curr_rooms);
    for room in lost_rooms {
        memory.rooms.remove(room);
        warn!("Lost room {}", room);
    }

    for room_name in curr_rooms {
        let room_data = memory.rooms.get_mut(&room_name);
        let room_data = match room_data {
            Some(room_data) => room_data,
            None => {
                let Some(new_room_data) = RoomData::from(room_name) else {
                    warn!("Unable to construct RoomData for {}", room_name);
                    continue;
                };
                memory.rooms.try_insert(room_name, new_room_data).ok().unwrap()
            },
        };

        let prev_stage = mem::take(&mut room_data.stage);
        room_data.stage = room_data.find_current_stage();

        if room_data.stage > prev_stage {
            info!("Promoted room {room_name} from stage {prev_stage:?} to stage {:?}", room_data.stage);
        } else if room_data.stage < prev_stage {
            info!("Demoted room {room_name} from stage {prev_stage:?} to stage {:?}", room_data.stage);
        }

        room_data.update(&mut memory.shared);
    }
}