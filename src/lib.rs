use log::*;
use screeps::game;
use wasm_bindgen::prelude::*;

use crate::{creeps::do_creeps, memory::Memory, movement::{update_movement_tick_end, update_movement_tick_start}, spawn::do_spawns, tower::do_towers};

mod logging;
mod names;
mod memory;
mod harvester;
mod planning;
mod tower;
mod movement;
mod claimer;
mod spawn;
mod creeps;
mod callbacks;

static INIT_LOGGING: std::sync::Once = std::sync::Once::new();

#[wasm_bindgen(js_name = loop)]
pub fn game_loop() {
    INIT_LOGGING.call_once(|| {
        logging::setup_logging(logging::Debug);
    });

    info!("=== Starting tick {} ===", game::time());

    let mut memory = Memory::screeps_deserialize();
    update_movement_tick_start();

    do_spawns(&mut memory);
    do_creeps(&mut memory);

    do_towers();

    update_movement_tick_end();

    memory.handle_callbacks();
    memory.screeps_serialize();
}