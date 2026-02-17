#![feature(map_try_insert)]
#![feature(variant_count)]

#![allow(clippy::enum_glob_use)]
#![allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
#![allow(clippy::cast_sign_loss, clippy::cast_precision_loss )]

use log::info;
use screeps::game;
use wasm_bindgen::prelude::*;

use crate::{creeps::do_creeps, memory::Memory, spawn::do_spawns, tower::do_towers};

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

    do_creeps(&mut mem);

    mem.messages.spawn.flush();
    do_spawns(&mut mem);

    do_towers();

    mem.movement.update_tick_end();

    mem.tick_times.push_front(game::cpu::get_used());
    if mem.tick_times.len() > 500 { mem.tick_times.pop_back(); }

    mem.handle_callbacks();
    mem.screeps_serialize();

    visuals::draw();
}