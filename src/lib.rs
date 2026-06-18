#![feature(variant_count)]
#![feature(trait_alias)]
#![feature(int_roundings)]
#![feature(min_specialization)]
#![feature(auto_traits)]
#![feature(negative_impls)]

#![allow(clippy::enum_glob_use)]
#![allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
#![allow(clippy::cast_sign_loss, clippy::cast_precision_loss )]

use std::cmp::Reverse;

use getrandom::register_custom_getrandom;
use itertools::Itertools;
use log::info;
use rand::{RngCore, SeedableRng, rngs::StdRng};
use screeps::{StructureLink, game};
use wasm_bindgen::prelude::*;

use crate::{colony::planning::planned_ref::ResolvableStructureRef, creeps::do_creeps, domain_traits::EnergyStoreAccessors, memory::Memory, spawn::{do_spawns, handle_incoming_creeps}, tower::do_towers};

mod logging;
mod names;
mod memory;
mod tower;
mod spawn;
mod creeps;
mod callbacks;
mod utils;
mod colony;
mod pathfinding;
mod visuals;
mod statemachine;
mod commands;
mod tasks;
mod safeid;
mod movement;
mod domain_traits;
mod new_tasks;

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
    info!("=== Starting tick {} (L[{:.1}], M[{:.1}], S[{:.1}]) Bucket: {} ===", game::time(), 
        mem.get_average_tick_rate_over(500), 
        mem.get_average_tick_rate_over(100),
        mem.get_average_tick_rate_over(10),
        game::cpu::bucket()
    );

    handle_incoming_creeps(&mut mem);

    update_coordinators(&mut mem);
    let tugboat_requests = do_creeps(&mut mem);

    do_spawns(&mut mem, tugboat_requests);

    do_towers();
    do_links(&mut mem);

    mem.tick_times.push_front(game::cpu::get_used());
    if mem.tick_times.len() > 500 { mem.tick_times.pop_back(); }

    mem.handle_callbacks();
    mem.screeps_serialize();

    visuals::draw();
}

#[expect(clippy::unnecessary_wraps)]
fn custom_getrandom(buf: &mut [u8]) -> Result<(), getrandom::Error> {
    let mut rng = StdRng::seed_from_u64(js_sys::Math::random().to_bits());
    rng.fill_bytes(buf);
    Ok(())
}
register_custom_getrandom!(custom_getrandom);

fn update_coordinators(mem: &mut Memory) {
    for colony in mem.colonies.view_all() {
        let creep_stops = mem.get_creep_stops(colony.name);

        mem.truck_coordinators.entry(colony.name).or_default().update(colony.plan, &colony.room, creep_stops);
        mem.fabricator_coordinators.entry(colony.name).or_default().update(&colony.room, colony.buffer);
    }
}

fn do_links(mem: &mut Memory) {
    for colony in mem.colonies.view_all(){
        let central_link: Option<StructureLink> = colony.plan.center.link.resolve();
        let Some(central_link) = central_link else { continue };

        let source_links: Vec<StructureLink> = colony.plan.sources.values()
            .filter_map(|plan| {
                let link = plan.link.resolve()?;

                let link_energy = link.used_energy_capacity();
                let container_energy = plan.container.resolve().map_or(0, |container| container.used_energy_capacity());

                Some((link, link_energy + container_energy))
            }).sorted_by_key(|(_, energy)| Reverse(*energy))
            .map(|(link, _)| link)
            .collect_vec();

        for source_link in source_links {
            if source_link.used_energy_capacity() > 400
                && central_link.free_energy_capacity() > 50 {
                    source_link.transfer_energy(&central_link, None).ok();
                    break;
                }
        }
    }
}