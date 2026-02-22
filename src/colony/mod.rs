use std::collections::{HashSet, hash_map};

use itertools::Itertools;
use js_sys::JsString;
use screeps::{Flag, HasPosition, OwnedStructureProperties, Position, Room, RoomName, Store, StructureContainer, StructureController, StructureStorage, Transferable, Withdrawable, find, game};
use serde::{Deserialize, Serialize};
use log::{info, warn};
use tap::Tap;

use crate::{colony::{planning::{plan::ColonyPlan, planned_ref::ResolvableStructureRef}, steps::ColonyStep}, commands::{Command, handle_commands, pop_command}, memory::Memory, statemachine::StateMachineTransition, visuals::{RoomDrawerType, draw_in_room_replaced}};

pub mod planning;
pub mod steps;

#[derive(Serialize, Deserialize)]
pub struct ColonyData {
    pub room_name: RoomName,
    pub plan: ColonyPlan
}

impl ColonyData {
    pub fn room(&self) -> Option<Room> {
        game::rooms().get(self.room_name)
    }

    pub fn controller(&self) -> Option<StructureController> {
        self.room()?.controller()
    }

    pub fn level(&self) -> u8 {
        self.controller().map_or(0, |controller| controller.level())
    }

    pub fn buffer(&self) -> Option<ColonyBuffer> {
        if let Some(storage) = self.plan.center.storage.resolve() { 
            Some(ColonyBuffer::Storage(storage))
        } else { self.plan.center.container_storage.resolve().map(ColonyBuffer::Container) }
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

    handle_commands(mem, |command, mem| {
        let Command::ResetColony { room: name } = command else { return false; };
        let Ok(name) = RoomName::new(name) else { return true; };
        mem.colonies.remove(&name);
        true
    });

    let owned_rooms = game::rooms().entries()
        .filter(|(_, room)| {
            if let Some(controller) = room.controller() { controller.my() }
            else { false }
        }).map(|(name, _)| name).collect_vec();

    if owned_rooms.len() == 1 {
        let room = owned_rooms.first().unwrap();
        let room = game::rooms().get(*room).unwrap();
        let controller = room.controller().unwrap();

        if controller.level() == 1 && controller.progress().unwrap() == 0
            && room.find(find::MY_CREEPS, None).is_empty() 
            && room.find(find::MY_STRUCTURES, None).len() == 2
            && room.find(find::MY_CONSTRUCTION_SITES, None).is_empty()
            && room.find(find::FLAGS, None).is_empty() {
                let spawn = room.find(find::MY_SPAWNS, None).into_iter().next().unwrap();
                spawn.pos().create_flag(Some(&JsString::from("Center")), None, None).ok();
            }
    }

    let claim_rooms = find_claim_flags().into_iter()
        .map(|flag| flag.pos().room_name());

    let curr_rooms: HashSet<_> = owned_rooms.into_iter().chain(claim_rooms).collect();
    let prev_rooms: HashSet<_> = mem.colonies.keys().copied().collect();

    let lost_rooms = prev_rooms.difference(&curr_rooms);
    for room in lost_rooms {
        mem.colonies.remove(room);
        mem.truck_coordinators.remove(room);
        warn!("Lost room {room}");
    }

    for name in curr_rooms {
        if let hash_map::Entry::Vacant(e) = mem.colonies.entry(name) {
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
                if pop_command(Command::MigrateColony { room: name.to_string() }) {
                    info!("Migrating {name}");
                    diff.migrate(name);
                } else {
                    diff.draw(name);

                    let plan_clone = plan.clone();
                    draw_in_room_replaced(name, RoomDrawerType::Plan, move |visuals| plan_clone.draw_until(visuals, None));
                    warn!("Plan for {name} is not compatible with current layout");
                    continue;
                }
            }

            let plan = plan.tap_mut(|plan| plan.adapt_build_times_to(&room));

            e.insert((ColonyData { room_name: room.name(), plan}, ColonyStep::default()));
        }

        if pop_command(Command::ResetColonyStep { room: name.to_string() }) {
            mem.colonies.get_mut(&name).unwrap().1 = ColonyStep::default();
        }

        if pop_command(Command::VisualizePlan { room: name.to_string(), animate: false }) {
            let plan_clone = mem.colonies.get(&name).unwrap().0.plan.clone();
            draw_in_room_replaced(name, RoomDrawerType::Plan, move |visuals| plan_clone.draw_until(visuals, None));
        }

        if pop_command(Command::VisualizePlan { room: name.to_string(), animate: true }) {
            let plan_clone = mem.colonies.get(&name).unwrap().0.plan.clone();
            plan_clone.draw_progression(name);
        }


        let (colony_data, step) = mem.colonies.get_mut(&name).unwrap();
        step.transition(&name, colony_data, &mut ());

        info!("{} is at step {:?}", name, step);
    }
}