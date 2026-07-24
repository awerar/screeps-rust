use std::{cmp::Reverse, collections::{HashMap, HashSet}};

use itertools::Itertools;
use screeps::{Direction, HasPosition, Position, Room, RoomXY, Terrain, find};
use anyhow::anyhow;

use crate::{colony::{planner::{floodfill::{DiagonalWalkableNeighs, FloodFill, OrthogonalWalkableNeighs, WalkableNeighs}, state::{ColonyPlanner, PlannedStructure}}, steps::ColonyStep}, pathfinding};

pub struct CenterPlanner {
    flood_fill: FloodFill<DiagonalWalkableNeighs>,
    roads_utility_increases: HashMap<RoomXY, Vec<ColonyStep>>
}

impl CenterPlanner {
    pub fn new(planner: &ColonyPlanner, center: RoomXY) -> Self {
        Self {
            flood_fill: FloodFill::new(vec![center], planner.room.get_terrain()),
            roads_utility_increases: HashMap::new()
        }
    }

    pub fn next_structure_pos(&mut self, planner: &ColonyPlanner, step: ColonyStep) -> anyhow::Result<RoomXY> {
        for (_, pos) in self.flood_fill.by_ref() {
            if planner.pos2structure.contains_key(&pos) { continue; }

            let road_neighs: Vec<_> = Direction::iter()
                .filter(|dir| dir.is_orthogonal())
                .filter_map(|dir| pos.checked_add_direction(*dir))
                .filter(|neigh| planner.terrain.get(neigh.x.u8(), neigh.y.u8()) != Terrain::Wall)
                .collect();

            for road_neigh in road_neighs {
                let road_utility_increases = self.roads_utility_increases.entry(road_neigh).or_default();
                road_utility_increases.push(step);
            }

            return Ok(pos);
        }

        Err(anyhow!("No more positions in center"))
    }

    pub fn plan_structure(&mut self, planner: &mut ColonyPlanner, step: ColonyStep, structure: PlannedStructure) -> anyhow::Result<()> {
        let pos = self.next_structure_pos(planner, step)?;
        planner.plan_structure(pos, step, structure)
    }

    pub fn plan_roads(self, planner: &mut ColonyPlanner) {
        for (road_pos, increases) in self.roads_utility_increases {
            let Some(plan_step) = increases.into_iter().sorted().nth(2) else { continue; };
            planner.plan_road(road_pos, plan_step);
        }
    }
}

pub fn plan_extensions_towers_observer(planner: &mut ColonyPlanner, center_planner: &mut CenterPlanner) -> anyhow::Result<()> {
    for controller_level in 1_u8..=8 {
        if controller_level == 8 {
            center_planner.plan_structure(planner, ColonyStep::BuildLvl8, PlannedStructure::Observer)?;
        }

        let step = ColonyStep::first_at_level(controller_level);
        let plan_extensions = planner.count_left_for(PlannedStructure::Extension, step);
        let plan_towers = planner.count_left_for(PlannedStructure::Tower, step);

        let mut avaliable_positions: HashSet<_> = (0..(plan_extensions + plan_towers)).map(|_| center_planner.next_structure_pos(planner, step)).collect::<Result<_, _>>()?;
        let mut towers = planner.structures2pos.get(&PlannedStructure::Tower).cloned().unwrap_or_default();
        let mut new_towers = Vec::new();

        for _ in 0..plan_towers {
            let tower = avaliable_positions.iter().sorted().max_by_key(|pos| {
                towers.iter()
                    .map(|other| u32::from(other.get_range_to(**pos)))
                    .sum::<u32>()
            }).copied().unwrap();

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

const MIN_ENTRANCE_DIST: usize = 8;
const MIN_CANDIDATE_DIST: u8 = 4;
pub fn find_center(room: &Room) -> RoomXY {
    let center_flag = room.find(find::FLAGS, None).into_iter()
        .find(|flag| flag.name().to_lowercase().contains("center"));
    if let Some(center_flag) = center_flag { return center_flag.pos().xy() }

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
        .sorted_by_key(|(dist, pos)| (Reverse(*dist), *pos))
        .dedup_by(|(d1, p1), (d2, p2)| *d1 == *d2 && p1.get_range_to(*p2) <= MIN_CANDIDATE_DIST)
        .take(5)
        .map(|(_, pos)| pos);
        //.inspect(|candidate| { let (x,y) = (candidate.x.u8(), candidate.y.u8()); draw_in_room(room.name(), move |visual| visual.circle(x as f32, y as f32, Some(CircleStyle::default().radius(0.35).fill("#469ff2")))); });

    candidates.min_by_key(|candidate| {
        let candidate_pos = Position::new(candidate.x, candidate.y, room.name());

        let mut points_of_interest = Vec::new();

        points_of_interest.extend(room.find(find::SOURCES, None).into_iter().map(|source| source.pos()));
        points_of_interest.extend(room.find(find::DEPOSITS, None).into_iter().map(|deposit| deposit.pos()));
        points_of_interest.push(room.controller().unwrap().pos());

        points_of_interest.into_iter()
            .map(|poi| pathfinding::search(candidate_pos, poi, 1).path().len())
            .sum::<usize>()
    })//.inspect(|best| { let (x,y) = (best.x.u8(), best.y.u8()); draw_in_room(room.name(), move |visual| visual.circle(x as f32, y as f32, Some(CircleStyle::default().radius(0.5).fill("#46f263")))); })
    .unwrap()
}
