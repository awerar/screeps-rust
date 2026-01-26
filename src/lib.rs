use log::*;
use screeps::{constants::Part, game, prelude::*};
use wasm_bindgen::prelude::*;

use crate::{harvester::{HarvesterState, do_harvester_creep}, memory::{Memory, Role, deserialize_memory, serialize_memory}, names::get_new_creep_name};

mod logging;
mod names;
mod memory;
mod harvester;

static INIT_LOGGING: std::sync::Once = std::sync::Once::new();

#[wasm_bindgen(js_name = loop)]
pub fn game_loop() {
    INIT_LOGGING.call_once(|| {
        logging::setup_logging(logging::Debug);
    });

    info!("=== Starting tick {} ===", game::time());

    let mut memory = deserialize_memory();

    do_spawns(&memory);
    memory = do_creeps(memory);

    serialize_memory(memory);
}

fn do_spawns(memory: &Memory) {
    if game::creeps().keys().count() >= memory.source_distribution.max_creeps() { return; }

    for spawn in game::spawns().values() {
        let body = [Part::Move, Part::Move, Part::Carry, Part::Work];
        if spawn.room().unwrap().energy_available() >= body.iter().map(|p| p.cost()).sum() {
            let name = get_new_creep_name();
            info!("Spawning new creep: {name}");

            if let Err(err) = spawn.spawn_creep(&body, &name) {
                warn!("Couldn't spawn creep: {}", err);
            }
        }
    }
}

fn do_creeps(mut memory: Memory) -> Memory {
    for creep in game::creeps().values() {
        let role = memory.creeps.entry(creep.name()).or_insert_with(||
            Role::Worker(HarvesterState::Idle)
        );
        
        match role {
            Role::Worker(state) => {
                let new_state = do_harvester_creep(&creep, state.clone(), &mut memory.source_distribution);
                if let Some(new_state) = new_state {
                    *state = new_state;
                } else {
                    warn!("Creep {} failed", creep.name());
                }
            },
        };
    }

    memory
}