use std::collections::HashMap;

use itertools::Itertools;
use log::*;
use screeps::{CircleStyle, CostMatrix, FindPathOptions, HasPosition, Path, Position, RoomCoordinate, RoomPosition, StructureType, action_error_codes::RoomPositionCreateConstructionSiteErrorCode, find, game, pathfinder::SingleRoomCostResult};
use serde::{Deserialize, Serialize};
use serde_json_any_key::*;
use wasm_bindgen::prelude::wasm_bindgen;

extern crate serde_json_path_to_error as serde_json;

const HALF_TIME: f32 = 100.0;
const ROAD_THRESHOLD: f32 = 7.5;

static TICK_DECAY: std::sync::LazyLock<f32> = std::sync::LazyLock::new(|| 0.5_f32.powf(1.0 / HALF_TIME));

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


#[derive(Serialize, Deserialize)]
struct TileUsage {
    usage: f32,
    last_update_tick: u32
}

impl Default for TileUsage {
    fn default() -> Self {
        Self { usage: 0.0, last_update_tick: game::time() }
    }
}

impl TileUsage {
    fn update(&mut self) -> f32 {
        if self.last_update_tick == game::time() { return self.usage; }

        self.usage *= TICK_DECAY.powi((game::time() - self.last_update_tick) as i32);
        self.last_update_tick = game::time();
        self.usage
    }

    pub fn add_usage(&mut self) -> f32 {
        self.update();
        self.usage += 1.0;
        self.usage
    }
}


#[derive(Serialize, Deserialize, Default)]
pub struct RoadPlan {
    #[serde(with = "any_key_map")]
    tile_usage: HashMap<Position, TileUsage>
}

impl RoadPlan {
    pub fn update_plan(&mut self) {
        for creep in game::creeps().values() {
            let usage = self.tile_usage.entry(creep.pos()).or_default().add_usage();
            if usage > ROAD_THRESHOLD {
                match creep.pos().create_construction_site(StructureType::Road, None) {
                    Ok(()) => info!("Creating road at {}", creep.pos()),
                    Err(RoomPositionCreateConstructionSiteErrorCode::InvalidTarget) => (),
                    Err(err) => warn!("Couldn't create road at {}: {}", creep.pos(), err),
                }
            }
        }

        #[cfg(true)]
        { // Usage visualization
            for (pos, usage) in self.tile_usage.iter_mut() {
                let usage = usage.update();

                let visual = game::rooms().get(pos.room_name()).unwrap().visual();
                visual.circle(
                    pos.x().u8().into(), 
                    pos.y().u8().into(), 
                    Some(CircleStyle::default().radius(0.5 * (usage / ROAD_THRESHOLD).min(1.0)))
                );
            }
        }
    }
}
