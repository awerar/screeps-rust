use std::collections::HashSet;

use itertools::Itertools;
use log::*;
use screeps::{CostMatrix, FindPathOptions, HasPosition, Path, Position, Room, RoomCoordinate, RoomName, StructureObject, StructureProperties, StructureType, Terrain, find, game, look, pathfinder::SingleRoomCostResult};
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
pub fn delete_roads_in(room_name: String) {
    let Some(room) = RoomName::new(&room_name).ok()
        .and_then(|room_name| game::rooms().get(room_name)) else {

        error!("Unkown room");
        return;
    };

    let roads = room.find(find::STRUCTURES, None).into_iter()
        .filter(|structure| structure.structure_type() == StructureType::Road);

    for road in roads {
        let StructureObject::StructureRoad(road) = road else { unreachable!() };
        road.destroy().ok();
    }
}

#[wasm_bindgen]
pub fn plan_main_roads_in_wasm(room_name: String) {
    if RoomName::new(&room_name).ok()
        .and_then(|room_name| game::rooms().get(room_name))
        .map(|room | plan_main_roads_in(&room)).is_none() {
        
        error!("Unable to plan main roads")
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

struct RoadPlan {
    new_roads: HashSet<Position>,
    old_roads: HashSet<Position>,
    cost_matrix: CostMatrix,
    room: Room
}

impl RoadPlan {
    fn new(room: Room) -> RoadPlan {
        let mut old_roads: HashSet<_> = room.find(find::STRUCTURES, None).into_iter()
            .filter(|structure| structure.structure_type() == StructureType::Road)
            .map(|structure| structure.pos()).collect();

        let road_sites: HashSet<_> = room.find(find::MY_CONSTRUCTION_SITES, None).into_iter()
            .filter(|site| site.structure_type() == StructureType::Road)
            .map(|site| site.pos()).collect();
        old_roads.extend(road_sites);

        let terrain = room.get_terrain();
        let cost_matrix = CostMatrix::new();
        for x in 0..50 {
            for y in 0..50 {
                let pos = Position::new(
                    RoomCoordinate::new(x).unwrap(),
                    RoomCoordinate::new(y).unwrap(),
                    room.name()
                );

                let cost = if old_roads.contains(&pos) { 3 }
                else if pos.look_for(look::STRUCTURES).unwrap().len() > 0 { 255 }
                else { 
                    match terrain.get(x, y) {
                        Terrain::Plain => 4,
                        Terrain::Swamp => 10,
                        Terrain::Wall => 255,
                    }
                };

                cost_matrix.set(x, y, cost);
            }
        }

        RoadPlan {
            old_roads,
            new_roads: HashSet::new(),
            cost_matrix: cost_matrix,
            room,
        }
    }

    fn plan_road(&mut self, pos: &Position) -> bool {
        if self.new_roads.contains(pos) { return false; }

        self.cost_matrix.set(pos.x().u8(), pos.y().u8(), 3);
        self.new_roads.insert(*pos);

        true
    }

    fn plan_road_between(&mut self, point1: &Position, point2: &Position) {
        let options = FindPathOptions::<fn(_, CostMatrix) -> SingleRoomCostResult, SingleRoomCostResult>::default()
            .ignore_creeps(true)
            .cost_callback(|_, _| SingleRoomCostResult::CostMatrix(self.cost_matrix.clone()));

        let path = point1.find_path_to(point2, Some(options));

        let Path::Vectorized(path) = path else { panic!("Path is supposed to be vectorized"); };

        let mut pos = point1.pos();
        for (i, step) in path.iter().enumerate() {
            pos.offset(step.dx, step.dy);

            if i > 0 && i < path.len() - 1 {
                self.plan_road(&pos );
            }
        }
    }

    fn plan_roads_around(&mut self, pos: &Position) {
        let terrain = self.room.get_terrain();

        for dx in -1..=1 {
            for dy in -1..=1 {
                if dx == 0 && dy == 0 { continue; }

                let pos = *pos + (dx, dy);

                if terrain.get(pos.x().u8(), pos.y().u8()) == Terrain::Wall { continue; }
                self.plan_road(&pos);
            }
        }
    }

    fn execute(self) {
        let construct_roads = self.new_roads.difference(&self.old_roads);
        let destroy_roads = self.old_roads.difference(&self.new_roads);

        for pos in construct_roads {
            pos.create_construction_site(StructureType::Road, None).ok();
        }

        for pos in destroy_roads {
            let roads = pos.look_for(look::STRUCTURES).unwrap().into_iter()
                .filter(|structure| structure.structure_type() == StructureType::Road);

            for road in roads {
                info!("Destroying road at {}", pos);
                road.destroy().ok();
            }

            let sites = pos.look_for(look::CONSTRUCTION_SITES).unwrap().into_iter()
                .filter(|structure| structure.structure_type() == StructureType::Road);

            for site in sites {
                info!("Destroying road construction site at {}", pos);
                site.remove().ok();
            }
        }
    }
}

pub fn plan_main_roads_in(room: &Room) {
    let sources: Vec<_> = room.find(find::SOURCES, None).into_iter().map(|source| source.pos()).collect();
    let fill_structure_types: HashSet<_> = HashSet::from([StructureType::Spawn, StructureType::Controller, StructureType::Tower, StructureType::Extension]);

    let fill_positions: Vec<_> = room.find(find::MY_STRUCTURES, None).into_iter()
        .filter(|structure| fill_structure_types.contains(&structure.structure_type()))
        .map(|structure| structure.pos()).collect();

    let mut plan = RoadPlan::new(room.clone());
    for source in &sources {
        plan.plan_roads_around(source);
    }

    plan.plan_roads_around(&room.controller().unwrap().pos());

    for (point1, point2) in sources.iter().cartesian_product(fill_positions.iter()) {
        plan.plan_road_between(point1, point2);
    }

    for (point1, point2) in fill_positions.iter().tuple_combinations() {
        plan.plan_road_between(point1, point2);
    }

    plan.execute();
}