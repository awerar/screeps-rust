use std::sync::LazyLock;

use log::*;
use screeps::{constants::Part, game, prelude::*};
use wasm_bindgen::prelude::*;

use crate::{harvester::{HarvesterState, do_harvester_creep}, memory::{Memory, Role, deserialize_memory, serialize_memory}, names::get_new_creep_name};

mod logging;
mod names;
mod memory;
mod harvester;
mod planning;

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
    memory.road_plan.update_plan();

    serialize_memory(memory);
}

const HARVESTER_TEMPLATE: LazyLock<Vec<Part>> = LazyLock::new(|| vec![Part::Carry, Part::Move, Part::Work]);


fn scale_body(template: &Vec<Part>, min_parts: Option<usize>, energy: u32) -> Option<Vec<Part>> {
    let mut counts: Vec<usize> = vec![0; template.len()];
    let mut cost = 0;

    let min_parts = min_parts.unwrap_or(template.len());

    loop {
        for (i, part )in template.iter().enumerate() {
            cost += part.cost();

            if cost > energy {
                let body: Vec<_> = template.iter()
                    .zip(counts.into_iter())
                    .flat_map(|(part, count)| vec![part.clone(); count].into_iter())
                    .collect();
                
                if body.len() > min_parts {
                    return Some(body);
                } else {
                    return None;
                }
            }

            counts[i] += 1;
        }
    }
}

fn do_spawns(memory: &Memory) {
    if game::creeps().keys().count() >= memory.source_distribution.max_creeps() { return; }

    for spawn in game::spawns().values() {
        let room = spawn.room().unwrap();

        let energy = room.energy_capacity_available();
        let body = scale_body(&HARVESTER_TEMPLATE, None, energy).unwrap();

        if room.energy_available() >= energy {
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
                    warn!("Creep {} failed. Idling.", creep.name());
                    *state = HarvesterState::Idle;
                }
            },
        };
    }

    memory
}