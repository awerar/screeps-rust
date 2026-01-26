use log::*;
use screeps::{constants::Part, game, prelude::*};
use wasm_bindgen::prelude::*;

use crate::{harvester::{HarvesterData, do_harvester_creep}, memory::{Memory, Role, deserialize_memory, serialize_memory}, names::get_new_creep_name};

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

    do_spawns(&mut memory);
    do_creeps(&mut memory);

    serialize_memory(memory);
}

fn do_spawns(memory: &mut Memory) {
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

fn do_creeps(memory: &mut Memory) {
    for creep in game::creeps().values() {
        let role = memory.creeps.entry(creep.name()).or_insert_with(||
            Role::Worker(HarvesterData { harvesting: true, target: None })
        );
        
        match role {
            Role::Worker(data) => {
                let result = do_harvester_creep(&creep, &mut memory.source_distribution, data);
                if result.is_none() {
                    warn!("Creep {} failed", creep.name());
                }
            },
        };
    }
}