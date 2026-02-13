use std::{cmp::Reverse, collections::HashSet};

use log::*;
use itertools::Itertools;
use screeps::{CircleStyle, Direction, HasId, HasPosition, Position, Room, RoomCoordinate, RoomName, RoomXY, Terrain, find, game};
use unionfind::HashUnionFindByRank;
use wasm_bindgen::prelude::wasm_bindgen;

use crate::{colony::planning::{steps::{ColonyState, Level1State}, floodfill::{FloodFill, OrthogonalWalkableNeighs, WalkableNeighs}, plan::ColonyPlan, planner::{CenterPlanner, ColonyPlanner, PlannedStructure}}, pathfinding, visuals::draw_in_room};

mod planner;
pub mod steps;
mod visuals;
pub mod plan;
mod floodfill;

#[wasm_bindgen]
pub fn visualize_plan_for(room: &str) {
    ColonyPlan::create_for(game::rooms().get(RoomName::new(room).unwrap()).unwrap()).unwrap();
}

impl ColonyPlan {
    pub fn create_for(room: Room) -> Result<Self, String> {
        use ColonyState::*;
        use Level1State::*;

        let mut planner = ColonyPlanner::new(room.clone());
        let center = find_center(room.clone());
        planner.plan_structure(center + Direction::Right, Level1(BuildContainerStorage), PlannedStructure::ContainerStorage)?;

        let mut center_planner = CenterPlanner::new(&planner, center);

        center_planner.plan_structure(&mut planner, Level4, PlannedStructure::Storage)?;
        center_planner.plan_structure(&mut planner, Level1(BuildSpawn), PlannedStructure::MainSpawn)?;
        center_planner.plan_structure(&mut planner, Level5, PlannedStructure::CentralLink)?;
        center_planner.plan_structure(&mut planner, Level6, PlannedStructure::Terminal)?;
        center_planner.plan_structure(&mut planner, Level3, PlannedStructure::Tower)?;

        let harvester_positions = plan_sources(&mut planner, center)?;

        plan_extensions_towers_observer(&mut planner, &mut center_planner)?;
        
        center_planner.plan_roads(&mut planner)?;

        let controller = room.controller().unwrap().pos().xy();
        planner.plan_road_between(center, controller, Level1(BuildArterialRoads))?;

        for source in harvester_positions {
            planner.plan_road_between(source, center, Level1(BuildArterialRoads))?;
        }

        for deposit in room.find(find::MINERALS, None) {
            planner.plan_structure(deposit.pos().xy(), Level6, PlannedStructure::Extractor)?;
            planner.plan_road_between(center, deposit.pos().xy(), Level6)?;

            let container_pos = deposit.pos().xy().neighbors().into_iter()
                .filter(|neigh| planner.roads.contains_key(neigh))
                .next().ok_or("Unable to find road around deposit")?;
            planner.plan_structure(container_pos, Level6, PlannedStructure::MineralContainer(deposit.id()))?;
        }

        ensure_connectivity(&mut planner, center)?;

        planner.compile().inspect(|plan| plan.draw_progression(room.name()))
    }
}

fn plan_sources(planner: &mut ColonyPlanner, center: RoomXY) -> Result<Vec<RoomXY>, String> {
    use ColonyState::*;
    use Level1State::*;

    let mut harvester_positions = Vec::new();
    for source in planner.room.find(find::SOURCES, None) {
        let source_pos = source.pos().xy();
        let source_id = source.id();

        let path = planner.find_path_between(source_pos, center, Level1(BuildArterialRoads));

        let harvest_pos = path.get(0).ok_or("Path to source had zero elements")?;
        let harvester_pos = RoomXY::new(
            RoomCoordinate::new(harvest_pos.x as u8).unwrap(), 
            RoomCoordinate::new(harvest_pos.y as u8).unwrap()
        );

        planner.plan_structure(harvester_pos, Level1(BuildSourceContainers), PlannedStructure::SourceContainer(source_id))?;
        planner.plan_road(harvester_pos, Level1(BuildArterialRoads))?;

        let slots = harvester_pos.neighbors().into_iter()
            .filter(|neigh| planner.is_free_at(*neigh))
            .collect_vec()
            .into_iter();

        let main_road_pos = path.get(1).ok_or("Path to source had one element")?;
        let main_road_pos = RoomXY::new(
            RoomCoordinate::new(main_road_pos.x as u8).unwrap(), 
            RoomCoordinate::new(main_road_pos.y as u8).unwrap()
        );

        planner.plan_structure_earliest(main_road_pos, PlannedStructure::SourceSpawn(source_id))?;

        let mut slots = slots.filter(|slot| *slot != main_road_pos);
        let link_slot = slots.next().ok_or("No slots for link around source")?;
        planner.plan_structure_earliest(link_slot, PlannedStructure::SourceLink(source_id))?;

        for slot in slots {
            planner.plan_structure_earliest(slot, PlannedStructure::SourceExtension(source_id))?;
        }

        harvester_positions.push(harvester_pos);
    }

    Ok(harvester_positions)
}

fn plan_extensions_towers_observer(planner: &mut ColonyPlanner, center_planner: &mut CenterPlanner) -> Result<(), String> {
    for controller_level in 1..=8 {
        if controller_level == 8 {
            center_planner.plan_structure(planner, ColonyState::Level8, PlannedStructure::Observer)?;
        }

        let step = ColonyState::first_at_level(controller_level as u8).unwrap();
        let plan_extensions = planner.count_left_for(PlannedStructure::Extension, step);
        let plan_towers = planner.count_left_for(PlannedStructure::Tower, step);

        let mut avaliable_positions: HashSet<_> = (0..(plan_extensions + plan_towers)).map(|_| center_planner.next_structure_pos(&planner, step)).collect::<Result<_, _>>()?;
        let mut towers = planner.structures2pos.get(&PlannedStructure::Tower).cloned().unwrap_or_default();
        let mut new_towers = Vec::new();

        for _ in 0..plan_towers {
            let tower = avaliable_positions.iter().max_by_key(|pos| {
                towers.iter()
                    .map(|other| other.get_range_to(**pos) as u32)
                    .sum::<u32>()
            }).cloned().unwrap();

            avaliable_positions.remove(&tower);
            towers.insert(tower);
            new_towers.push(tower);
        }

        for pos in avaliable_positions {
            planner.plan_structure(pos, step, PlannedStructure::Extension)?;
        }

        for pos in new_towers {
            planner.plan_structure(pos, step, PlannedStructure::Tower)?;
        }
    }

    Ok(())
}

fn ensure_connectivity(planner: &mut ColonyPlanner, center: RoomXY) -> Result<(), String> {
    let mut network = HashUnionFindByRank::new(vec![center]).unwrap();

    for step in ColonyState::iter() {
        let new_roads: Vec<_> = planner.roads.iter()
            .filter(|(_, road_step)| step == **road_step)
            .map(|(pos, _)| pos)
            .cloned()
            .collect();

        for new_road in &new_roads {
            network.add(*new_road).map_err(|e| e.to_string())?;
            for neigh in new_road.neighbors() {
                if network.find_shorten(&neigh).is_some() {
                    network.union_by_rank(new_road, &neigh).map_err(|e| e.to_string())?;
                }
            }
        }

        for new_road in &new_roads {
            if network.find_shorten(new_road) != network.find_shorten(&center) {
                planner.plan_road_between(center, *new_road, step)?;
                network.union_by_rank(new_road, &center).map_err(|e| e.to_string())?;
            }
        }

        let new_structures: Vec<_> = planner.structures.iter()
            .filter(|(_, road_step)| step == **road_step)
            .map(|(pos, _)| pos)
            .filter(|pos| !matches!(planner.pos2structure[*pos], PlannedStructure::SourceContainer(_)))
            .cloned()
            .collect();

        for new_structure in new_structures {
            if new_structure == center { continue; }
            if new_structure.neighbors().into_iter().any(|neigh| network.find_shorten(&neigh).is_some()) { continue; }
            debug!("Connecting {center} and {new_structure}");
            planner.plan_road_between(center, new_structure, step)?;
        }
    }

    Ok(())
}

const MIN_ENTRANCE_DIST: usize = 8;
const MIN_CANDIDATE_DIST: u8 = 4;
fn find_center(room: Room) -> RoomXY {
    let exits = room.find(find::EXIT, None).into_iter()
        .map(|pos| Position::from(pos).xy());

    let entrance_blocks = FloodFill::<WalkableNeighs>::new(exits, room.get_terrain())
        .take_while(|(dist, _)| *dist <= MIN_ENTRANCE_DIST)
        .map(|(_, pos)| pos);
        //.inspect(|candidate| { let (x,y) = (candidate.x.u8(), candidate.y.u8()); draw_in_room(room.name(), move |visual| visual.circle(x as f32, y as f32, Some(CircleStyle::default().radius(0.2).fill("#f53636")))); });

    let wall_blocks = (0..50).cartesian_product(0..50)
        .filter(|(x, y)| room.get_terrain().get(*x, *y) == Terrain::Wall)
        .map(|(x, y)| RoomXY::try_from((x, y)).unwrap());

    let candidates = FloodFill::<OrthogonalWalkableNeighs>::new(wall_blocks.chain(entrance_blocks), room.get_terrain())
        .sorted_by_key(|(dist, _)| Reverse(*dist))
        .dedup_by(|(d1, p1), (d2, p2)| *d1 == *d2 && p1.get_range_to(*p2) <= MIN_CANDIDATE_DIST)
        .take(5)
        .map(|(_, pos)| pos)
        .inspect(|candidate| { let (x,y) = (candidate.x.u8(), candidate.y.u8()); draw_in_room(room.name(), move |visual| visual.circle(x as f32, y as f32, Some(CircleStyle::default().radius(0.35).fill("#469ff2")))); });

    candidates.min_by_key(|candidate| {
        let candidate_pos = Position::new(candidate.x, candidate.y, room.name());

        let mut points_of_interest = Vec::new();

        points_of_interest.extend(room.find(find::SOURCES, None).into_iter().map(|source| source.pos()));
        points_of_interest.extend(room.find(find::DEPOSITS, None).into_iter().map(|deposit| deposit.pos()));
        points_of_interest.push(room.controller().unwrap().pos());

        points_of_interest.into_iter()
            .map(|poi| pathfinding::search(candidate_pos, poi, 1).path().len())
            .sum::<usize>()
    }).inspect(|best| { let (x,y) = (best.x.u8(), best.y.u8()); draw_in_room(room.name(), move |visual| visual.circle(x as f32, y as f32, Some(CircleStyle::default().radius(0.5).fill("#46f263")))); })
    .unwrap()
}