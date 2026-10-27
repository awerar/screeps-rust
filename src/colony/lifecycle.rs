use std::collections::{HashSet, hash_map};

use itertools::Itertools;
use js_sys::JsString;
use screeps::{HasPosition, OwnedStructureProperties, Room, RoomName, find, game};
use log::{info, warn};

use crate::{colony::{ColonyView, plan::{ColonyPlan, get_all_structures_in}, steps::ColonyStep}, commands::{Command, handle_commands, pop_command}, memory::Memory, statemachine::step, visuals::{RoomDrawerType, draw_in_room_replaced}};

pub fn update_colonies(mem: &mut Memory) {
    info!("Updating rooms...");

    handle_commands(|command| {
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

    let lost_colonies = prev_colonies.difference(&curr_colonies);
    for room in lost_colonies {
        mem.colonies.0.remove(room);
        mem.truck_coordinators.remove(room);
        mem.fabricator_coordinators.remove(room);
        warn!("Lost colony {room}");
    }

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
                let center_pos = spawn.pos() + (0, 1);
                center_pos.create_flag(Some(&JsString::from("Center")), None, None).ok();

                return;
            }
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

            if !plan.handle_incompatible(&room) { continue; }

            let step = plan.get_latest_compatible_step_for(&room);
            e.insert((plan, step));
        }

        if pop_command(Command::ResetColonyStep { room: name.to_string() }) {
            let (plan, step) = mem.colonies.0.get_mut(&name).unwrap();
            
            if plan.handle_incompatible(&room) {
                *step = plan.get_latest_compatible_step_for(&room);
            }
        }

        if pop_command(Command::VisualizePlan { room: name.to_string(), animate: false }) {
            let plan_clone = mem.colonies.0.get(&name).unwrap().0.clone();
            draw_in_room_replaced(name, RoomDrawerType::Plan, move |visuals| plan_clone.draw_until(visuals, None));
        }

        if pop_command(Command::VisualizePlan { room: name.to_string(), animate: true }) {
            let plan_clone = mem.colonies.0.get(&name).unwrap().0.clone();
            plan_clone.draw_progression(name);
        }

        let (plan, stp) = mem.colonies.0.get_mut(&name).unwrap();
        let view = ColonyView::new(room.clone(), plan, *stp);
        step(stp, |stp| stp.update(&room, &view));

        info!("{name} is at step {stp:?}");
    }
}

impl ColonyPlan {
    fn handle_incompatible(&self, room: &Room) -> bool {
        let diff = self.diff_with(room);
        if diff.compatible() { return true }

        let name = room.name();
        if pop_command(Command::MigrateColony { room: name.to_string() }) {
            info!("Migrating {name}");
            diff.migrate(name);

            true
        } else {
            diff.draw(name);

            let plan_clone = self.clone();
            draw_in_room_replaced(name, RoomDrawerType::Plan, move |visuals| plan_clone.draw_until(visuals, None));
            warn!("Plan for {name} is not compatible with current layout");
            
            false
        }
    }
}

impl ColonyPlan {
    pub fn get_latest_compatible_step_for(&self, room: &Room) -> ColonyStep {
        let controller = room.controller().unwrap();
        let all_structures: HashSet<_> = get_all_structures_in(room).into_keys().collect();

        for (step, step_plan) in self.steps.iter().sorted_by_key(|(a, _)| **a).rev() {
            if step.controller_level() <= controller.level() 
                && step_plan.accumulated_structures.is_subset(&all_structures) {
                    return *step
            }
        }

        unreachable!()
    }
}