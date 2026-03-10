use std::{collections::{HashMap, HashSet, hash_map}, fmt::Display};

use js_sys::JsString;
use screeps::{Flag, HasPosition, OwnedStructureProperties, Position, ResourceType, Room, RoomName, Store, StructureContainer, StructureController, StructureStorage, Transferable, Withdrawable, find, game};
use log::{info, warn};
use serde::{Deserialize, Serialize};
use tap::Tap;

use crate::{colony::{planning::{plan::ColonyPlan, planned_ref::ResolvableStructureRef}, steps::ColonyStep}, commands::{Command, handle_commands, pop_command}, memory::Memory, statemachine::StateMachineTransition, visuals::{RoomDrawerType, draw_in_room_replaced}};

pub mod planning;
pub mod steps;

#[derive(Serialize, Deserialize, Default)]
pub struct Colonies(HashMap<RoomName, (ColonyPlan, ColonyStep)>);

pub struct ColonyView<'mem> {
    pub plan: &'mem ColonyPlan,
    #[expect(unused)] pub step: ColonyStep,
    pub name: RoomName,
    pub room: Room,
    pub controller: StructureController,
    pub buffer: Option<ColonyBuffer>
}

impl Display for ColonyView<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

impl<'mem> ColonyView<'mem> {
    pub fn new(room: Room, plan: &'mem ColonyPlan, step: ColonyStep) -> Self {
        let buffer = plan.center.storage.resolve().map(ColonyBuffer::Storage)
            .or_else(|| plan.center.container_storage.resolve().map(ColonyBuffer::Container));

        ColonyView { 
            plan, 
            step, 
            name: room.name(), 
            controller: room.controller().expect("Every colony should have a controller"), 
            room, 
            buffer
        }
    }
}

impl Colonies {
    pub fn view(&self, name: RoomName) -> Option<ColonyView<'_>> {
        let (plan, step) = self.0.get(&name)?;
        Some(ColonyView::new(game::rooms().get(name)?, plan, *step))
    }

    pub fn view_all(&self) -> impl Iterator<Item = ColonyView<'_>> {
        self.0.keys().filter_map(|name| self.view(*name))
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

    pub fn energy(&self) -> u32 {
        self.store().get_used_capacity(Some(ResourceType::Energy))
    }

    pub fn energy_capacity_left(&self) -> u32 {
        self.store().get_free_capacity(Some(ResourceType::Energy)).try_into().unwrap_or(0)
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

pub fn update_colonies(mem: &mut Memory) {
    info!("Updating rooms...");

    handle_commands(mem, |command, mem| {
        let Command::ResetColony { room: name } = command else { return false; };
        let Ok(name) = RoomName::new(name) else { return true; };
        mem.colonies.0.remove(&name);
        true
    });

    let prev_colonies: HashSet<_> = mem.colonies.0.keys().copied().collect();
    let curr_colonies: HashSet<_> = game::rooms().entries()
        .filter(|(_, room)| {
            if let Some(controller) = room.controller() { controller.my() }
            else { false }
        }).map(|(name, _)| name).collect();

    if curr_colonies.len() == 1 {
        let room = curr_colonies.iter().next().unwrap();
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

    let lost_colonies = prev_colonies.difference(&curr_colonies);
    for room in lost_colonies {
        mem.colonies.0.remove(room);
        mem.truck_coordinators.remove(room);
        mem.fabricator_coordinators.remove(room);
        warn!("Lost colony {room}");
    }

    for name in curr_colonies {
        let room = game::rooms().get(name).unwrap();

        if let hash_map::Entry::Vacant(e) = mem.colonies.0.entry(name) {
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

            e.insert((plan, ColonyStep::default()));
        }

        if pop_command(Command::ResetColonyStep { room: name.to_string() }) {
            mem.colonies.0.get_mut(&name).unwrap().1 = ColonyStep::default();
        }

        if pop_command(Command::VisualizePlan { room: name.to_string(), animate: false }) {
            let plan_clone = mem.colonies.0.get(&name).unwrap().0.clone();
            draw_in_room_replaced(name, RoomDrawerType::Plan, move |visuals| plan_clone.draw_until(visuals, None));
        }

        if pop_command(Command::VisualizePlan { room: name.to_string(), animate: true }) {
            let plan_clone = mem.colonies.0.get(&name).unwrap().0.clone();
            plan_clone.draw_progression(name);
        }


        let (plan, step) = mem.colonies.0.get_mut(&name).unwrap();
        let view = ColonyView::new(room.clone(), plan, *step);
        step.transition(&room, &mut &view);

        info!("{name} is at step {step:?}");
    }
}