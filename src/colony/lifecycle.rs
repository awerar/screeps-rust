use std::collections::HashSet;

use js_sys::JsString;
use screeps::{HasPosition, Room, find, game};
use log::{error, info, warn};

use crate::{colony::{Colonies, plan::ColonyPlan, registry::is_owned_room, steps::ColonyStep}, commands::{Command, pop_command}, statemachine::step, visuals::{RoomDrawerType, clear_room_visual, draw_in_room_replaced}};

pub fn update_colonies(colonies: &mut Colonies) {
    let curr_colonies: HashSet<_> = game::rooms().keys()
        .filter(|name| is_owned_room(*name))
        .collect();

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

    for room_name in curr_colonies {
        let room = game::rooms().get(room_name).unwrap();

        let name = if let Some(name) = colonies.lookup(room_name) { name } else {
            let plan = ColonyPlan::create_for(&room);
            let Ok(plan) = plan else {
                let Err(err) = plan else { unreachable!() };
                warn!("Unable to create plan for {room_name}: {err}");
                continue;
            };

            if !plan.handle_incompatible(&room) { continue; }

            let step = plan.get_latest_compatible_step_for(&room);
            let Some(name) = colonies.insert(room_name, plan, step) else { continue; };
            name
        };

        if pop_command(Command::ResetColonyStep { room: room_name.to_string() }) {
            let colony = colonies.get_mut(name);

            if colony.plan.handle_incompatible(&room) {
                colony.step = colony.plan.get_latest_compatible_step_for(&room);
            }
        }

        if pop_command(Command::VisualizePlan { room: room_name.to_string(), animate: false }) {
            let plan_clone = colonies.get(name).plan.clone();
            draw_in_room_replaced(room_name, RoomDrawerType::Plan, move |visuals| plan_clone.draw_until(visuals, None));
        }

        if pop_command(Command::VisualizePlan { room: room_name.to_string(), animate: true }) {
            let plan_clone = colonies.get(name).plan.clone();
            plan_clone.draw_progression(room_name);
        }

        let colony = colonies.get_mut(name);
        let result = {
            let view = colony.view();
            step(colony.step, |stp| stp.update(&room, &view))
        };

        match result {
            Ok(new_step) => colony.step = new_step,
            Err(e) => error!("Colony {name} failed, falling back to the default step: {e:#}"),
        }
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
            clear_room_visual(name, RoomDrawerType::Diff);

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
        /*let controller = room.controller().unwrap();
        let all_structures: HashSet<_> = get_all_structures_in(room).into_keys().collect();

        for (step, step_plan) in self.steps.iter().sorted_by_key(|(a, _)| **a).rev() {
            if step.controller_level() <= controller.level() 
                && step_plan.accumulated_structures.is_subset(&all_structures) {
                    return *step
            }
        }

        unreachable!()*/

        // TODO: FIX
        ColonyStep::first()
    }
}