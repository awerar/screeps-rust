#![feature(map_try_insert)]
#![feature(variant_count)]

use log::*;
use screeps::game;
use wasm_bindgen::prelude::*;

use crate::{creeps::do_creeps, memory::Memory, spawn::do_spawns, tower::do_towers};

mod logging;
mod names;
mod memory;
mod planning;
mod tower;
mod movement;
mod spawn;
mod creeps;
mod callbacks;
mod colony;
mod remote_build;

static INIT_LOGGING: std::sync::Once = std::sync::Once::new();

#[wasm_bindgen(js_name = loop)]
pub fn game_loop() {
    INIT_LOGGING.call_once(|| {
        logging::setup_logging(logging::Info);
    });

    if game::cpu::bucket() >= screeps::constants::PIXEL_CPU_COST as i32 {
        info!("Generating pixel!");
        game::cpu::generate_pixel().ok();
    }

    if game::cpu::bucket() < 100 {
        info!("Waiting for buckets {}/100", game::cpu::bucket());
        return;
    }

    let mut memory = Memory::screeps_deserialize();
    memory.shared.movement.update_tick_start();

    info!("=== Starting tick {} (500: {:.1}, 100: {:.1}, 10: {:.1}) ===", game::time(), 
        memory.get_average_tick_rate_over(500), 
        memory.get_average_tick_rate_over(100),
        memory.get_average_tick_rate_over(10)
    );

    do_spawns(&mut memory);
    do_creeps(&mut memory);

    do_towers();

    memory.shared.movement.update_tick_end();

    memory.tick_times.push_front(game::cpu::get_used());
    if memory.tick_times.len() > 500 { memory.tick_times.pop_back(); }

    memory.handle_callbacks();
    memory.screeps_serialize();
}