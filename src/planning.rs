use itertools::Itertools;
use log::*;
use screeps::{CostMatrix, FindPathOptions, HasPosition, Path, Position, RoomCoordinate, RoomPosition, StructureType, find, game, look, pathfinder::SingleRoomCostResult};
use wasm_bindgen::prelude::wasm_bindgen;

extern crate serde_json_path_to_error as serde_json;

#[wasm_bindgen]
pub fn clear_pending_roads() {
    for room in game::rooms().values() {
        for site in room.find(find::CONSTRUCTION_SITES, None) {
            if site.progress() == 0 && site.structure_type() == StructureType::Road {
                site.remove().ok();
            }
        }
    }
}

#[wasm_bindgen]
pub fn plan_spawn_extensions() {
    let spawn = game::spawns().values().next().unwrap();
    let room = spawn.room().unwrap();
    let controller = room.controller().unwrap();
    let max_spawn_extensions = StructureType::Extension.controller_structures(controller.level() as u32);
    
    let origin = spawn.pos();
    let mut curr_placed = 0;
    for radius in 1_i32..5 {
        for dx in -radius..=radius {
            'inner: for dy in -radius..=radius {
                if curr_placed >= max_spawn_extensions { return; }

                if (dx.abs() + dy.abs()) % 2 != 0 { continue; }
                if dx.abs().max(dy.abs()) < radius { continue; }

                let pos = origin + (dx, dy);
                if let Ok(sites) = pos.look_for(look::CONSTRUCTION_SITES) {
                    for site in sites {
                        if site.structure_type() == StructureType::Extension {
                            curr_placed += 1;
                            continue 'inner;
                        }
                        site.remove().ok();
                    }
                }

                match pos.create_construction_site(StructureType::Extension, None) {
                    Ok(_) => curr_placed += 1,
                    Err(err) => warn!("Couldn't place extension at {}: {}", pos, err),
                }
            }
        }
    }
}

#[wasm_bindgen]
pub fn plan_main_roads() {
    let room = game::spawns().values().next().unwrap().room().unwrap();

    let sources_pos: Vec<_> = room.find(find::SOURCES, None).into_iter()
        .map(|source| source.pos()).collect();

    let mut targets_pos = vec![room.controller().unwrap().pos()];
    targets_pos.extend(room.find(find::MY_SPAWNS, None).into_iter().map(|spawn| spawn.pos()));

    for (p1, p2) in sources_pos.into_iter().cartesian_product(targets_pos.into_iter()) {
        let options = FindPathOptions::<fn(_, CostMatrix) -> SingleRoomCostResult, SingleRoomCostResult>::default()
            .ignore_creeps(true);
            //.swamp_cost(2);

        let path = room.find_path(
            &RoomPosition::from(p1),
            &RoomPosition::from(p2),
            Some(options)
        );

        let Path::Vectorized(path) = path else { panic!("Path is supposed to be vectorized"); };
        for step in path {
            let pos = Position::new(
                RoomCoordinate::new(step.x as u8).unwrap(), 
                RoomCoordinate::new(step.y as u8).unwrap(),
                room.name()
            );

            pos.create_construction_site(StructureType::Road, None).ok();
        }
    }
}