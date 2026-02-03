use std::{collections::{HashMap, HashSet, VecDeque}, sync::LazyLock};

use itertools::Itertools;
use log::*;
use screeps::{CostMatrix, Direction, FindPathOptions, HasPosition, Path, Position, Room, RoomCoordinate, StructureProperties, StructureType, Terrain, find, look, pathfinder::SingleRoomCostResult};

use crate::colony::ColonyConfig;

extern crate serde_json_path_to_error as serde_json;

const CENTER_STRUCTURE_TYPES: LazyLock<Vec<StructureType>> = LazyLock::new(|| {
    use StructureType::*;
    vec![Spawn, Storage, Extension, Tower]
});

pub fn plan_center_in(colony_config: &ColonyConfig) -> Option<()> {
    let room = colony_config.room()?;
    let controller_level = room.controller()?.level() as u32;

    let already_built: Vec<_> = room.find(find::MY_STRUCTURES, None).into_iter()
        .map(|structure| structure.structure_type())
        .collect();

    let already_sited: Vec<_> = room.find(find::MY_CONSTRUCTION_SITES, None).into_iter()
        .map(|site| site.structure_type())
        .collect();

    let mut already_planned_count = HashMap::new();
    for structure_type in already_built.into_iter().chain(already_sited.into_iter()) {
        *already_planned_count.entry(structure_type).or_default() += 1;
    }

    let plan_queue: Vec<_> = CENTER_STRUCTURE_TYPES.iter()
        .flat_map(|structure_type| {
            let total = structure_type.controller_structures(controller_level);
            let already_planned = already_planned_count.get(structure_type).unwrap_or(&0);
            let left = (total - *already_planned).max(0);

            (0..left).map(|_| structure_type.clone())
        }).collect();

    plan_center_structures_in(colony_config, plan_queue)
}

fn plan_center_structures_in(colony_config: &ColonyConfig, plan_queue: Vec<StructureType>) -> Option<()> {
    let mut plan_queue = VecDeque::from(plan_queue);

    'plan_loop: for radius in 1_u32..5 {
        let mut direction = Direction::Left;
        let mut curr_pos = colony_config.center + ((radius % 2) as i32, radius as i32);
        let mut positions = HashSet::new();

        while !positions.contains(&curr_pos) {
            if (curr_pos.x().u8() + curr_pos.y().u8()) % 2 == 0 {
                positions.insert(curr_pos);
            }
            
            if colony_config.center.get_range_to(curr_pos + direction) > radius {
                direction = direction.multi_rot(2);
            }

            curr_pos = curr_pos + direction;
        }

        'pos_loop: for pos in positions {
            let Some(structure) = plan_queue.front() else { break 'plan_loop };

            let sites = pos.look_for(look::CONSTRUCTION_SITES).ok()?;
            for site in sites {
                if site.structure_type() == StructureType::Road {
                    site.remove().ok();
                } else {
                    continue 'pos_loop;
                }
            }

            let structures = pos.look_for(look::STRUCTURES).ok()?;
            for structure in structures {
                if structure.structure_type() == StructureType::Road {
                    structure.destroy().ok();
                } else {
                    continue 'pos_loop;
                }
            }

            match pos.create_construction_site(*structure, None) {
                Ok(()) => { plan_queue.pop_front().unwrap(); },
                Err(err) => {
                    warn!("Unable to place {} at {}: {}", structure, pos, err);
                },
            }
        }
    }

    if plan_queue.len() > 0 {
        error!("Unable to plan all structures within given space");
        None
    } else {
        Some(())
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

    let mut fill_positions: Vec<_> = room.find(find::MY_STRUCTURES, None).into_iter()
        .filter(|structure| fill_structure_types.contains(&structure.structure_type()))
        .map(|structure| structure.pos()).collect();

    fill_positions.extend(room.find(find::CONSTRUCTION_SITES, None).into_iter()
        .filter(|site| fill_structure_types.contains(&site.structure_type()))
        .map(|site| site.pos()));

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