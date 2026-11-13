use std::{cmp::Reverse, collections::{BinaryHeap, HashMap, HashSet, VecDeque}};

use itertools::Itertools;
use log::info;
use screeps::{HasPosition, MOVE_COST_PLAIN, MOVE_COST_SWAMP, Position, Room, RoomTerrain, RoomXY, StructureType, Terrain, find};
use anyhow::{bail, anyhow};

use crate::{colony::{planner::{floodfill::{FloodFill, OrthogonalWalkableNeighs, WalkableNeighs}, state::{ColonyPlanner, LifeTime, PlannedImpassableStructure}}, steps::ColonyStep}, utils::GetXYGood};

struct XYAncestry {
    dist: u32,
    parents: Vec<RoomXY>
}

impl XYAncestry {
    fn new(dist: u32, parents: Vec<RoomXY>) -> Self {
        Self { dist, parents }
    }
}

trait GetTileCostAtXy { fn get_tile_cost_at_xy(&self, xy: RoomXY) -> u32; }
impl GetTileCostAtXy for RoomTerrain {
    fn get_tile_cost_at_xy(&self, xy: RoomXY) -> u32 {
        match self.get_xy_good(xy) {
            Terrain::Plain => 1,
            Terrain::Swamp => 5,
            Terrain::Wall => 255,
        }
    }
}

pub struct CenterPlanner {
    terrain: RoomTerrain,

    ancestries: HashMap<RoomXY, XYAncestry>,
    structure_lifetimes: HashMap<RoomXY, LifeTime>,

    road_pq: BinaryHeap<Reverse<(u32, RoomXY)>>,
    explored_roads: HashSet<RoomXY>,
    structure_queue: VecDeque<(u32, RoomXY)>,
    

    center: RoomXY
}

impl CenterPlanner {
    pub fn new(center: RoomXY, room: &Room) -> Self {
        Self {
            road_pq: BinaryHeap::from([Reverse((0, center))]),
            ancestries: HashMap::from([(center, XYAncestry::new(0, Vec::new()))]),
            structure_queue: VecDeque::new(),
            structure_lifetimes: HashMap::new(),
            explored_roads: HashSet::new(),
            center,
            terrain: room.get_terrain()
        }
    }

    pub fn next_structure_pos(&mut self, planner: &mut ColonyPlanner, step: ColonyStep) -> anyhow::Result<RoomXY> {
        loop {
            while let Some((dist, pos)) = self.structure_queue.pop_front() {
                if !planner.is_free_at(pos) { continue; }
                if !planner.can_block_at(pos) { 
                    self.road_pq.push(Reverse((dist, pos)));

                    continue; 
                }

                planner.reserve_block(pos)?;
                self.structure_lifetimes.insert(pos, LifeTime::permanent(step));
                return Ok(pos) 
            }

            let Some(Reverse((dist, road))) = self.road_pq.pop() else { bail!("Not enough roads"); };
            if !self.explored_roads.insert(road) { continue; }
            if !planner.is_not_blocked(road) { continue; }

            for neigh in road.neighbors() {
                let neigh_dist = dist + self.terrain.get_tile_cost_at_xy(neigh);

                if self.is_on_road_grid(neigh) {
                    self.road_pq.push(Reverse((neigh_dist, neigh)));
                } else {
                    self.structure_queue.push_back((neigh_dist, neigh));
                }

                self.ancestries.entry(neigh).and_modify(|ancestry| {
                    if neigh_dist < ancestry.dist {
                        *ancestry = XYAncestry { dist: neigh_dist, parents: vec![road] }
                    } else if neigh_dist == ancestry.dist {
                        ancestry.parents.push(road);
                    }
                }).or_insert_with(|| XYAncestry { dist: neigh_dist, parents: vec![road] });
            }
        }
    }

    pub fn plan_structure(&mut self, planner: &mut ColonyPlanner, step: ColonyStep, structure: PlannedImpassableStructure) -> anyhow::Result<()> {
        let pos = self.next_structure_pos(planner, step)?;
        planner.plan_blocker(pos, step, structure.into())
    }

    pub fn plan_roads(self, planner: &mut ColonyPlanner) -> anyhow::Result<()> {
        for (pos, lt) in &self.structure_lifetimes {
            let lt = lt.clip_start(ColonyStep::BuildArterialRoads).unwrap();

            for ancestor in self.get_all_ancestors(*pos) {
                planner.plan_road(ancestor, lt)?;
            }
        }

        Ok(())
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

        d % 4 == 0 || (d % 4 == 2 && dx % 2 == 1)
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
const MIN_WALL_DIST: usize = 3;
pub fn find_center(room: &Room) -> anyhow::Result<RoomXY> {
    let terrain = room.get_terrain();

    let exits = room.find(find::EXIT, None).into_iter()
        .map(|pos| Position::from(pos).xy());

    let entrance_blocks = FloodFill::new(exits, WalkableNeighs::new(room.get_terrain()))
        .take_while(|(dist, _)| *dist <= MIN_ENTRANCE_DIST)
        .map(|(_, pos)| pos);
        //.inspect(|candidate| { let (x,y) = (candidate.x.u8(), candidate.y.u8()); draw_in_room(room.name(), move |visual| visual.circle(x as f32, y as f32, Some(CircleStyle::default().radius(0.2).fill("#f53636")))); });

    let wall_blocks = (0..50).cartesian_product(0..50)
        .filter(|(x, y)| terrain.get(*x, *y) == Terrain::Wall)
        .map(|(x, y)| RoomXY::try_from((x, y)).unwrap());

    let candidates: HashSet<_> = FloodFill::new(wall_blocks.chain(entrance_blocks), OrthogonalWalkableNeighs::new(room.get_terrain()))
        .filter(|(dist, _)| *dist >= MIN_WALL_DIST)
        .map(|(_, pos)| pos)
        .filter(|pos| terrain.get_xy_good(*pos) != Terrain::Swamp)
        .collect();

    let center_flag = room.find(find::FLAGS, None).into_iter()
        .find(|flag| flag.name().to_lowercase().contains("center"));

    if let Some(center_flag) = center_flag.map(|f| f.pos().xy()) { 
        if candidates.iter().find(|candidate| **candidate == center_flag).is_some() {
            return Ok(center_flag) 
        }
        
        info!("Found a center flag, but it is not a valid center");
    }

    let mut points_of_interest = Vec::new();

    points_of_interest.extend(room.find(find::SOURCES, None).into_iter().map(|source| (1, source.pos())));
    points_of_interest.extend(room.find(find::MINERALS, None).into_iter().map(|mineral| (2, mineral.pos())));
    points_of_interest.push((3, room.controller().unwrap().pos()));

    let mut candidate_penalties: HashMap<_, u32> = HashMap::new();

    for (weight, poi) in points_of_interest {
        let mut pq = BinaryHeap::new();
        let mut visited = HashSet::new();

        pq.push((Reverse(0), poi.xy()));

        while let Some((Reverse(dist), pos)) = pq.pop() {
            if !visited.insert(pos) { continue; }

            if candidates.contains(&pos) {
                *candidate_penalties.entry(pos).or_default() += dist * weight;
            }

            for neigh in pos.neighbors() {
                let delta = match terrain.get_xy_good(neigh) {
                    Terrain::Wall => continue,
                    Terrain::Plain => MOVE_COST_PLAIN,
                    Terrain::Swamp => MOVE_COST_SWAMP,
                };

                pq.push((Reverse(dist + delta), neigh));
            }
        }
    }

    candidate_penalties.into_iter()
        .sorted_by_key(|(pos, _)| *pos)
        .min_by_key(|(_, dist)| *dist)
        .map(|(pos, _)| pos)
        .ok_or(anyhow!("Unable to find a center"))
}
