use std::collections::HashSet;

use screeps::{Flag, HasPosition, OwnedStructureProperties, Position, Room, RoomName, Store, StructureContainer, StructureController, StructureStorage, Transferable, Withdrawable, game};
use serde::{Deserialize, Serialize};
use log::*;

use crate::{colony::{planning::plan::ColonyPlan, steps::ColonyStep}, memory::Memory};

pub mod planning;
mod steps;

#[derive(Serialize, Deserialize)]
pub struct ColonyData {
    pub room_name: RoomName,
    pub plan: ColonyPlan,
    pub step: ColonyStep
}

impl ColonyData {
    pub fn room(&self) -> Option<Room> {
        game::rooms().get(self.room_name)
    }

    pub fn controller(&self) -> Option<StructureController> {
        self.room()?.controller()
    }

    pub fn level(&self) -> u8 {
        self.controller().map(|controller| controller.level()).unwrap_or(0)
    }

    pub fn buffer(&self) -> Option<ColonyBuffer> {
        if let Some(storage) = self.plan.center.storage.resolve() { 
            Some(ColonyBuffer::Storage(storage))
        } else if let Some(container) = self.plan.center.container_storage.resolve() {
            Some(ColonyBuffer::Container(container))
        } else {
            None
        }
    }
}

pub enum ColonyBuffer {
    Container(StructureContainer),
    Storage(StructureStorage)
}

impl ColonyBuffer {
    pub fn withdrawable(&self) -> &dyn Withdrawable {
        match self {
            ColonyBuffer::Container(container) => container,
            ColonyBuffer::Storage(storage) => storage,
        }
    }

    pub fn transferable(&self) -> &dyn Transferable {
        match self {
            ColonyBuffer::Container(container) => container,
            ColonyBuffer::Storage(storage) => storage,
        }
    }

    pub fn store(&self) -> Store {
        match self {
            ColonyBuffer::Container(container) => container.store(),
            ColonyBuffer::Storage(storage) => storage.store(),
        }
    }
}

impl HasPosition for ColonyBuffer {
    #[doc = " Position of the object."]
    fn pos(&self) -> Position {
        match self {
            ColonyBuffer::Container(container) => container.pos(),
            ColonyBuffer::Storage(storage) => storage.pos(),
        }
    }
}

const CLAIM_FLAG_PREFIX: &str = "Claim";
fn find_claim_flags() -> Vec<Flag> {
    game::flags().entries()
        .filter(|(name, _)| name.starts_with(CLAIM_FLAG_PREFIX))
        .map(|(_, flag)| flag)
        .collect()
}

pub fn update_rooms(mem: &mut Memory) {
    info!("Updating rooms...");

    let owned_rooms = game::rooms().entries()
        .filter(|(_, room)| {
            if let Some(controller) = room.controller() { controller.my() }
            else { false }
        }).map(|(name, _)| name);


    let claim_rooms = find_claim_flags().into_iter()
        .map(|flag| flag.pos().room_name());

    let curr_rooms: HashSet<_> = owned_rooms.chain(claim_rooms).collect();
    let prev_rooms: HashSet<_> = mem.colonies.keys().cloned().collect();

    let lost_rooms = prev_rooms.difference(&curr_rooms);
    for room in lost_rooms {
        mem.colonies.remove(room);
        warn!("Lost room {}", room);
    }

    for name in curr_rooms {
        todo!();
        /*if !mem.colonies.contains_key(&name) {
            let Some(colony) = ColonyData::try_construct_from(name) else {
                error!("Unable to construct colony config for {name}");
                continue; 
            };
            mem.colonies.insert(name, colony);
        }

        let step = mem.colonies[&name].step.clone();
        mem.colonies.get_mut(&name).unwrap().steo = step.update(name, mem, 0);*/
    }
}