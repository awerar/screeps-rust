use std::{cmp::Reverse, collections::{BinaryHeap, HashMap, HashSet, VecDeque, hash_map}};

use itertools::Itertools;
use log::info;
use screeps::{Direction, HasPosition, Position, Room, RoomXY, StructureType, Terrain, find};
use anyhow::{bail, anyhow};

use crate::{colony::{planner::{floodfill::{FloodFill, OrthogonalWalkableNeighs, WalkableNeighs}, state::{ColonyPlanner, LifeTime, PlannedImpassableStructure, PlannedContainer}}, steps::ColonyStep}, pathfinding};

struct XYAncestry {
    dist: u32,
    parents: Vec<RoomXY>
}

impl XYAncestry {
    fn new(dist: u32, parents: Vec<RoomXY>) -> Self {
        Self { dist, parents }
    }
}

pub struct CenterPlanner {
    ancestries: HashMap<RoomXY, XYAncestry>,

    road_pq: BinaryHeap<Reverse<(u32, RoomXY)>>,
    structure_queue: VecDeque<(u32, RoomXY)>,
    structure_lifetimes: HashMap<RoomXY, LifeTime>,

    container_buffer_pos: RoomXY,
    center: RoomXY
}

// Algorithm:
// Expand road network outward via BFS and add structure spots as needed
// Seed are the points of the middle star
// If a spot can't be blocked, instead plan a road there
// For each structure, remember which roads first reach it earliest
// When planning roads, for each structure, make 
impl CenterPlanner {
    pub fn new(center: RoomXY) -> Self {
        let mut ancestries = HashMap::new();
        let mut road_pq = BinaryHeap::new();
        let mut structure_queue = VecDeque::new();

        structure_queue.push_back((1, center));
        ancestries.insert(center, XYAncestry::new(
            1, 
            Direction::iter().filter(|dir| dir.is_diagonal()).map(|dir| center + *dir).collect_vec()
        ));

        for dir in Direction::iter() {
            let pos = center + *dir;

            if dir.is_orthogonal() {
                structure_queue.push_back((1, pos));
                ancestries.insert(pos, XYAncestry::new(1, vec![ center + dir.rot_cw(), center + dir.rot_ccw() ]));
            } else {
                road_pq.push(Reverse((0, pos)));
                ancestries.insert(pos, XYAncestry::new(0, Vec::new()));
            }
        }

        Self {
            ancestries,
            road_pq,
            structure_queue,
            structure_lifetimes: HashMap::new(),
            container_buffer_pos: center + Direction::TopRight,
            center
        }
    }

    pub fn next_structure_pos(&mut self, planner: &mut ColonyPlanner, step: ColonyStep) -> anyhow::Result<RoomXY> {
        loop {
            while let Some((dist, pos)) = self.structure_queue.pop_front() {
                if !planner.is_free_at(pos) { 
                    self.road_pq.push(Reverse((dist, pos)));

                    continue; 
                }

                planner.block_checker.block(pos)?;
                self.structure_lifetimes.insert(pos, LifeTime::permanent(step));
                return Ok(pos) 
            }

            let Some(Reverse((dist, road))) = self.road_pq.pop() else { bail!("Not enough roads"); };

            for dir in Direction::iter() {
                let Some(neigh) = road.checked_add_direction(*dir) else { continue; };
                if !planner.is_free_at(neigh) { continue; }

                let is_road_neigh = self.is_on_road_grid(neigh);

                match self.ancestries.entry(neigh) {
                    hash_map::Entry::Vacant(entry) => {
                        if is_road_neigh {
                            self.road_pq.push(Reverse((dist + 1, neigh)));
                        } else {
                            self.structure_queue.push_back((dist + 1, neigh));
                        }

                        entry.insert(XYAncestry::new(dist + 1, vec![road]));
                    },
                    hash_map::Entry::Occupied(mut entry) => {
                        if entry.get().dist == dist + 1 {
                            entry.get_mut().parents.push(road);
                        }
                    },
                }
            }
        }
    }

    pub fn plan_structure(&mut self, planner: &mut ColonyPlanner, step: ColonyStep, structure: PlannedImpassableStructure) -> anyhow::Result<()> {
        let pos = self.next_structure_pos(planner, step)?;
        planner.plan_blocker(pos, step, structure.into())
    }

    pub fn plan_container_buffer(&mut self, planner: &mut ColonyPlanner, lt: LifeTime) -> anyhow::Result<()> { 
        self.structure_lifetimes.insert(self.container_buffer_pos, lt);
        planner.plan_container(self.container_buffer_pos, lt, PlannedContainer::ContainerBuffer)
    }

    pub fn plan_roads(self, planner: &mut ColonyPlanner) {
        for (pos, lt) in &self.structure_lifetimes {
            let lt = lt.clip_start(ColonyStep::BuildArterialRoads).unwrap();

            for ancestor in self.get_all_ancestors(*pos) {
                planner.plan_road(ancestor, lt);
            }
        }
    }

    fn get_all_ancestors(&self, xy: RoomXY) -> HashSet<RoomXY> {
        let immediate_parents= self.ancestries.get(&xy).unwrap().parents.clone();

        let mut ancestors = immediate_parents.iter().copied().collect::<HashSet<_>>();
        let mut stack = immediate_parents;

        while let Some(xy) = stack.pop() {
            let ancestry = self.ancestries.get(&xy).unwrap();
            
            for parent in &ancestry.parents {
                if ancestors.insert(*parent) {
                    stack.push(*parent);
                }
            }
        }

        ancestors
    }

    fn is_on_road_grid(&self, pos: RoomXY) -> bool {
        let (dx, dy) = pos - self.center;
        let (dx, dy) = (dx.abs(), dy.abs());
        let d = dx + dy;

        d % 4 == 2 || (d % 4 == 0 && dx % 2 == 1)
    }
}

pub fn plan_extensions_towers_observer(planner: &mut ColonyPlanner, center_planner: &mut CenterPlanner) -> anyhow::Result<()> {
    for controller_level in 1_u8..=8 {
        if controller_level == 8 {
            center_planner.plan_structure(planner, ColonyStep::BuildLvl8, PlannedImpassableStructure::Observer)?;
        }

        let step = ColonyStep::first_at_level(controller_level);
        let plan_extensions = planner.count_left_for(StructureType::Extension, step);
        let plan_towers = planner.count_left_for(StructureType::Tower, step);

        let mut avaliable_positions: HashSet<_> = (0..(plan_extensions + plan_towers)).map(|_| center_planner.next_structure_pos(planner, step)).collect::<Result<_, _>>()?;
        let mut towers: HashSet<_> = planner.find_all(PlannedImpassableStructure::Tower.into()).into_iter().collect();
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
            planner.plan_blocker(pos, step, PlannedImpassableStructure::Extension.into())?;
        }

        for pos in new_towers {
            planner.plan_blocker(pos, step, PlannedImpassableStructure::Tower.into())?;
        }
    }

    Ok(())
}

const MIN_ENTRANCE_DIST: usize = 8;
const MIN_CANDIDATE_DIST: u8 = 4;
const MIN_WALL_DIST: usize = 3;
pub fn find_center(room: &Room) -> anyhow::Result<RoomXY> {
    let exits = room.find(find::EXIT, None).into_iter()
        .map(|pos| Position::from(pos).xy());

    let entrance_blocks = FloodFill::new(exits, WalkableNeighs::new(room.get_terrain()))
        .take_while(|(dist, _)| *dist <= MIN_ENTRANCE_DIST)
        .map(|(_, pos)| pos);
        //.inspect(|candidate| { let (x,y) = (candidate.x.u8(), candidate.y.u8()); draw_in_room(room.name(), move |visual| visual.circle(x as f32, y as f32, Some(CircleStyle::default().radius(0.2).fill("#f53636")))); });

    let wall_blocks = (0..50).cartesian_product(0..50)
        .filter(|(x, y)| room.get_terrain().get(*x, *y) == Terrain::Wall)
        .map(|(x, y)| RoomXY::try_from((x, y)).unwrap());

    let all_candidates = FloodFill::new(wall_blocks.chain(entrance_blocks), OrthogonalWalkableNeighs::new(room.get_terrain()))
        .filter(|(dist, _)| *dist >= MIN_WALL_DIST)
        .collect_vec();

    let center_flag = room.find(find::FLAGS, None).into_iter()
        .find(|flag| flag.name().to_lowercase().contains("center"));
    if let Some(center_flag) = center_flag.map(|f| f.pos().xy()) { 
        if all_candidates.iter().find(|(_, candidate)| *candidate == center_flag).is_some() {
            return Ok(center_flag) 
        }
        
        info!("Found a center flag, but it is not a valid center");
    }

    let candidates = all_candidates.into_iter()
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
    .ok_or(anyhow!("No valid center candidate found"))
}
