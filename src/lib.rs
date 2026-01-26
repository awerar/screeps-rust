use log::*;
use screeps::{
    constants::{Part, ResourceType}, game, objects::Creep, prelude::*
};
use wasm_bindgen::prelude::*;

use crate::{memory::{HarvesterData, HarvesterTarget, Memory, Role, SourceDistribution, deserialize_memory, serialize_memory}, names::get_new_creep_name};

mod logging;
mod names;
mod memory;

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

fn do_harvester_creep(creep: &Creep, source_distribution: &mut SourceDistribution, data: &mut HarvesterData) -> Option<()> {
    if data.harvesting {
        if creep.store().get_free_capacity(None) == 0 {
            data.harvesting = false;
            data.target = None;
        }
    } else {
        if creep.store().get_used_capacity(None) == 0 {
            data.harvesting = true;
        }
    }

    if data.harvesting {
        if let Some((pos, source)) = source_distribution.get_assignmemnt(&creep) {
            let move_result = creep.move_to(pos);

            if creep.pos() == pos || move_result.is_err() {
                let source = source.resolve()?;
                creep.harvest(&source).ok();
            }
        } else {
            warn!("Creep {} has no assignment", creep.name())
        }
    } else {
        if data.target.is_none() {
            let room = creep.room()?;
            if room.energy_available() < room.energy_capacity_available() {
                data.target = Some(HarvesterTarget::Spawn(game::spawns().values().next()?.id()));
            } else {
                data.target = Some(HarvesterTarget::Controller(room.controller()?.id()));
            }
        }

        if let Some(target) = &data.target {
            match target {
                HarvesterTarget::Controller(target) => {
                    let target = target.resolve()?;
                    creep.move_to(&target).ok();

                    if creep.pos().is_near_to(target.pos()) {
                        creep.upgrade_controller(&target).ok();
                    }
                },
                HarvesterTarget::Spawn(target) => {
                    let target = target.resolve()?;
                    creep.move_to(&target).ok();

                    if creep.pos().is_near_to(target.pos()) {
                        creep.transfer(&target, ResourceType::Energy, None).ok();
                    }
                },
            }
        }
    }

    Some(())
}