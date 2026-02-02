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

static INIT_LOGGING: std::sync::Once = std::sync::Once::new();

#[wasm_bindgen(js_name = loop)]
pub fn game_loop() {
    INIT_LOGGING.call_once(|| {
        logging::setup_logging(logging::Debug);
    });

    info!("=== Starting tick {} ===", game::time());

    let mut memory = Memory::screeps_deserialize();
    memory.shared.movement.update_tick_start();

    do_spawns(&mut memory);
    do_creeps(&mut memory);

    do_towers();

    memory.shared.movement.update_tick_end();

    memory.handle_callbacks();
    memory.screeps_serialize();
}