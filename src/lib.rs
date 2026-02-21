#![feature(map_try_insert)]
#![feature(variant_count)]
#![feature(assert_matches)]
#![feature(trait_alias)]

#![allow(clippy::enum_glob_use)]
#![allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
#![allow(clippy::cast_sign_loss, clippy::cast_precision_loss )]

use log::info;
use screeps::{StructureLink, game};
use wasm_bindgen::prelude::*;

use crate::{colony::planning::planned_ref::ResolvableStructureRef, creeps::do_creeps, memory::Memory, spawn::do_spawns, tower::do_towers};

mod logging;
mod names;
mod memory;
mod tower;
mod movement;
mod spawn;
mod creeps;
mod callbacks;
mod remote_build;
mod utils;
mod messages;
mod colony;
mod pathfinding;
mod visuals;
mod statemachine;
mod commands;
mod tasks;

static INIT_LOGGING: std::sync::Once = std::sync::Once::new();

#[wasm_bindgen(js_name = loop)]
pub fn game_loop() {
    INIT_LOGGING.call_once(|| {
        logging::setup_logging(logging::Debug);
    });

    if game::cpu::bucket() >= screeps::constants::PIXEL_CPU_COST as i32 {
        info!("Generating pixel!");
        game::cpu::generate_pixel().ok();
    }

    if game::cpu::bucket() < 100 && game::cpu::tick_limit() != f64::INFINITY {
        info!("Waiting for buckets {}/100", game::cpu::bucket());
        return;
    }

    let mut mem = Memory::screeps_deserialize();
    mem.movement.update_tick_start();

    info!("=== Starting tick {} (L[{:.1}], M[{:.1}], S[{:.1}]) Bucket: {} ===", game::time(), 
        mem.get_average_tick_rate_over(500), 
        mem.get_average_tick_rate_over(100),
        mem.get_average_tick_rate_over(10),
        game::cpu::bucket()
    );

    update_truck_coordinators(&mut mem);
    do_creeps(&mut mem);

    mem.messages.trucks.flush();
    mem.messages.spawn.flush();
    do_spawns(&mut mem);

    do_towers();
    do_links(&mut mem);

    mem.movement.update_tick_end();

    mem.tick_times.push_front(game::cpu::get_used());
    if mem.tick_times.len() > 500 { mem.tick_times.pop_back(); }

    mem.handle_callbacks();
    mem.screeps_serialize();

    visuals::draw();
}

fn update_truck_coordinators(mem: &mut Memory) {
    for (colony, colony_data) in &mem.colonies {
        let Some(room) = game::rooms().get(*colony) else { continue; };
        mem.truck_coordinators.entry(*colony).or_default().update(&colony_data.plan, &room);
    }
}

fn do_links(mem: &mut Memory) {
    for colony in mem.colonies.values() {
        let central_link: Option<StructureLink> = colony.plan.center.link.resolve();
        let Some(central_link) = central_link else { continue };

        for source_plan in colony.plan.sources.source_plans.values() {
            let source_link: Option<StructureLink> = source_plan.link.resolve();
            let Some(source_link) = source_link else { continue };

            if source_link.store().get_used_capacity(Some(screeps::ResourceType::Energy)) >= 400
                && central_link.store().get_free_capacity(Some(screeps::ResourceType::Energy)) >= 400 {
                    source_link.transfer_energy(&central_link, Some(400)).ok();
                    break;
                }
        }
    }
}