use std::collections::HashSet;

use itertools::Itertools;
use js_sys::JsString;
use screeps::{Flag, HasPosition, OwnedStructureProperties, Position, Room, RoomName, Store, StructureContainer, StructureController, StructureStorage, Transferable, Withdrawable, find, game};
use serde::{Deserialize, Serialize};
use log::*;

use crate::{colony::{planning::plan::ColonyPlan, steps::ColonyStep}, commands::{Command, pop_command}, memory::Memory, statemachine::transition, visuals::{RoomDrawerType, draw_in_room_replaced}};

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
        }).map(|(name, _)| name).collect_vec();

    if owned_rooms.len() == 1 {
        let room = owned_rooms.iter().next().unwrap();
        let room = game::rooms().get(*room).unwrap();
        let controller = room.controller().unwrap();

        if controller.level() == 1 && controller.progress().unwrap() == 0 {
            if room.find(find::MY_CREEPS, None).len() == 0 
            && room.find(find::MY_STRUCTURES, None).len() == 2
            && room.find(find::MY_CONSTRUCTION_SITES, None).len() == 0
            && room.find(find::FLAGS, None).len() == 0 {
                let spawn = room.find(find::MY_SPAWNS, None).into_iter().next().unwrap();
                spawn.pos().create_flag(Some(&JsString::from("Center")), None, None).ok();
            }
        }
    }

    let claim_rooms = find_claim_flags().into_iter()
        .map(|flag| flag.pos().room_name());

    let curr_rooms: HashSet<_> = owned_rooms.into_iter().chain(claim_rooms).collect();
    let prev_rooms: HashSet<_> = mem.colonies.keys().cloned().collect();

    let lost_rooms = prev_rooms.difference(&curr_rooms);
    for room in lost_rooms {
        mem.colonies.remove(room);
        warn!("Lost room {}", room);
    }

    for name in curr_rooms {
        if !mem.colonies.contains_key(&name) {
            let Some(room) = game::rooms().get(name) else {
                warn!("Unable to plan for {name} due to lack of vision");
                continue;
            };

            let plan = ColonyPlan::create_for(&room);
            let Ok(plan) = plan else {
                let Err(err) = plan else { unreachable!() };
                warn!("Unable to create plan for {name}: {err}");
                continue;
            };

            let diff = plan.diff_with(&room);
            if !diff.compatible() {
                if pop_command(Command::MigrateRoom { room: name.to_string() }) {
                    info!("Migrating {}", name);
                    diff.migrate(name);
                } else {
                    diff.draw(name);

                    let plan_clone = plan.clone();
                    draw_in_room_replaced(name, RoomDrawerType::Plan, move |visuals| plan_clone.draw_until(visuals, None));
                    warn!("Plan for {name} is not compatible with current layout");
                    continue;
                }
            }

            mem.colonies.insert(name, ColonyData { 
                room_name: room.name(), 
                plan, 
                step: Default::default()
            });
        }

        if pop_command(Command::ResetColonyStep { room: name.to_string() }) {
            mem.colonies.get_mut(&name).unwrap().step = Default::default();
        }

        if pop_command(Command::VisualizePlan { room: name.to_string() }) {
            let plan_clone = mem.colonies.get(&name).unwrap().plan.clone();
            draw_in_room_replaced(name, RoomDrawerType::Plan, move |visuals| plan_clone.draw_until(visuals, None));
        }

        let step = mem.colonies[&name].step.clone();
        mem.colonies.get_mut(&name).unwrap().step = transition(&step, &name, mem);
    }
}