use std::{cmp::Reverse, collections::{BTreeMap, HashMap, HashSet, VecDeque}};

use log::*;
use itertools::Itertools;
use screeps::{CircleStyle, CostMatrix, CostMatrixSet, Direction, FindPathOptions, HasId, HasPosition, LineStyle, ObjectId, Path, Position, Room, RoomCoordinate, RoomName, RoomTerrain, RoomVisual, RoomXY, Source, Step, StructureType, Terrain, TextStyle, find, game, pathfinder::SingleRoomCostResult};
use serde::{Deserialize, Serialize};
use unionfind::HashUnionFindByRank;
use wasm_bindgen::prelude::wasm_bindgen;

use crate::{colony::{ColonyState, Level1State, State}, pathfinding, visuals::draw_in_room};

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Clone, Copy, Debug)]
pub enum PlannedStructure {
    MainSpawn,
    SourceSpawn(ObjectId<Source>),
    SourceContainer(ObjectId<Source>),
    SourceLink(ObjectId<Source>),
    SourceExtension(ObjectId<Source>),
    Extension,
    Storage,
    Terminal,
    ContainerStorage,
    Tower,
    CentralLink,
    Extractor,
}

impl PlannedStructure {
    fn walkable(&self) -> bool {
        use PlannedStructure::*;

        matches!(self, SourceContainer(_) | ContainerStorage)
    }

    fn structure_type(&self) -> StructureType {
        use StructureType::*;

        match self {
            PlannedStructure::MainSpawn => Spawn,
            PlannedStructure::SourceSpawn(_) => Spawn,
            PlannedStructure::SourceContainer(_) => Container,
            PlannedStructure::Extension => Extension,
            PlannedStructure::SourceExtension(_) => Extension,
            PlannedStructure::Storage => Storage,
            PlannedStructure::ContainerStorage => Container,
            PlannedStructure::Tower => Tower,
            PlannedStructure::CentralLink => Link,
            PlannedStructure::SourceLink(_) => Link,
            PlannedStructure::Terminal => Terminal,
            PlannedStructure::Extractor => Extractor,
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct ColonyPlanStep {
    new_roads: HashSet<RoomXY>,
    new_structures: HashMap<RoomXY, PlannedStructure>
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ColonyPlan {
    steps: HashMap<ColonyState, ColonyPlanStep>
}

#[wasm_bindgen]
pub fn visualize_plan_for(room: &str) {
    ColonyPlan::create_for(game::rooms().get(RoomName::new(room).unwrap()).unwrap()).unwrap();
}

impl ColonyPlan {
    pub fn create_for(room: Room) -> Result<Self, String> {
        use ColonyState::*;
        use Level1State::*;

        let mut planner = ColonyPlanner::new(room.clone());
        let center = Self::find_center(room.clone());

        let mut center_planner = CenterPlanner::new(&planner, center);

        center_planner.plan_structure(&mut planner, Level4, PlannedStructure::Storage)?;
        center_planner.plan_structure(&mut planner, Level1(BuildSpawn), PlannedStructure::MainSpawn)?;
        center_planner.plan_structure(&mut planner, Level1(BuildContainerStorage), PlannedStructure::ContainerStorage)?;
        center_planner.plan_structure(&mut planner, Level3, PlannedStructure::Tower)?;
        center_planner.plan_structure(&mut planner, Level5, PlannedStructure::CentralLink)?;

        let harvester_positions = Self::plan_sources(&mut planner, center)?;

        Self::plan_extensions_and_towers(&mut planner, &mut center_planner)?;
        
        center_planner.plan_roads(&mut planner)?;

        let controller = room.controller().unwrap().pos().xy();
        planner.plan_road_between(center, controller, Level1(BuildArterialRoads))?;

        for source in harvester_positions {
            planner.plan_road_between(source, center, Level1(BuildArterialRoads))?;
        }

        Self::ensure_connectivity(&mut planner, center)?;

        planner.compile().inspect(|plan| plan.draw_progression(room.name()))
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
            planner.plan_road(main_road_pos, Level1(BuildArterialRoads))?;

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

    fn plan_extensions_and_towers(planner: &mut ColonyPlanner, center_planner: &mut CenterPlanner) -> Result<(), String> {
        for controller_level in 1..=8 {
            let step = ColonyState::first_at_level(controller_level as u8).unwrap();
            let plan_extensions = planner.count_left_for(PlannedStructure::Extension, step.clone());
            let plan_towers = planner.count_left_for(PlannedStructure::Tower, step.clone());

            let mut avaliable_positions: HashSet<_> = (0..(plan_extensions + plan_towers)).map(|_| center_planner.next_structure_pos(&planner, step.clone())).collect::<Result<_, _>>()?;
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
                planner.plan_structure(pos, step.clone(), PlannedStructure::Extension)?;
            }

            for pos in new_towers {
                planner.plan_structure(pos, step.clone(), PlannedStructure::Tower)?;
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
                    planner.plan_road_between(center, *new_road, step.clone())?;
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
                planner.plan_road_between(center, new_structure, step.clone())?;
            }
        }

        Ok(())
    }

    const MIN_ENTRANCE_DIST: usize = 8;
    const MIN_CANDIDATE_DIST: u8 = 4;
    fn find_center(room: Room) -> RoomXY {
        let exits = room.find(find::EXIT, None).into_iter()
            .map(|pos| Position::from(pos).xy());

        let entrance_blocks = FloodFill::new(exits, WalkableNeighs(room.get_terrain()))
            .take_while(|(dist, _)| *dist <= Self::MIN_ENTRANCE_DIST)
            .map(|(_, pos)| pos);
            //.inspect(|candidate| { let (x,y) = (candidate.x.u8(), candidate.y.u8()); draw_in_room(room.name(), move |visual| visual.circle(x as f32, y as f32, Some(CircleStyle::default().radius(0.2).fill("#f53636")))); });

        let wall_blocks = (0..50).cartesian_product(0..50)
            .filter(|(x, y)| room.get_terrain().get(*x, *y) == Terrain::Wall)
            .map(|(x, y)| RoomXY::try_from((x, y)).unwrap());

        let candidates = FloodFill::new(wall_blocks.chain(entrance_blocks), OrthogonalWalkableNeighs(room.get_terrain()))
            .sorted_by_key(|(dist, _)| Reverse(*dist))
            .dedup_by(|(d1, p1), (d2, p2)| *d1 == *d2 && p1.get_range_to(*p2) <= Self::MIN_CANDIDATE_DIST)
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
}

#[derive(Clone, Copy)]
enum TilePathing {
    BuiltRoad,
    PlannedRoad,
    Plains,
    Swamp,
    Impassable
}

impl From<Terrain> for TilePathing {
    fn from(value: Terrain) -> Self {
        match value {
            Terrain::Plain => TilePathing::Plains,
            Terrain::Swamp => TilePathing::Swamp,
            Terrain::Wall => TilePathing::Impassable,
        }
    }
}

impl TilePathing {
    fn cost(&self) -> u8 {
        match self {
            TilePathing::BuiltRoad => 5,
            TilePathing::PlannedRoad => 6,
            TilePathing::Plains => 8,
            TilePathing::Swamp => 20,
            TilePathing::Impassable => 255,
        }
    }
}

pub struct ColonyPlanner {
    pub cost_matrix: CostMatrix,
    pub terrain: RoomTerrain,
    pub room: Room,

    pub roads: HashMap<RoomXY, ColonyState>,
    pub structures: HashMap<RoomXY, ColonyState>,

    pub pos2structure: HashMap<RoomXY, PlannedStructure>,
    pub structures2pos: HashMap<PlannedStructure, HashSet<RoomXY>>,
    pub structure_type_steps: HashMap<StructureType, BTreeMap<ColonyState, u32>>
}

impl ColonyPlanner {
    pub fn new(room: Room) -> Self {
        let terrain = room.get_terrain();
        let cost_matrix = CostMatrix::new();
        for x in 0..50 {
            for y in 0..50 {
                cost_matrix.set(x, y, TilePathing::from(terrain.get(x, y)).cost());
            }
        }

        ColonyPlanner { 
            cost_matrix, 
            terrain, 
            room,
            roads: HashMap::new(),
            structures: HashMap::new(),
            pos2structure: HashMap::new(), 
            structures2pos: HashMap::new(),
            structure_type_steps: HashMap::new()
        }
    }

    pub fn compile(self) -> Result<ColonyPlan, String> {
        let mut plan_steps = HashMap::new();

        for step in ColonyState::iter() {
            let mut plan_step = ColonyPlanStep::default();
            
            plan_step.new_roads.extend(self.roads.iter()
                .filter(|(_, road_step)| **road_step == step)
                .map(|(pos, _)| pos.clone())
            );

            plan_step.new_structures.extend(self.structures.iter()
                .filter(|(_, road_step)| **road_step == step)
                .map(|(pos, _)| (pos.clone(), self.pos2structure[pos]))
            );

            plan_steps.insert(step.clone(), plan_step);
        }

        Ok(ColonyPlan { steps: plan_steps })
    }

    pub fn count_left_for(&self, structure: PlannedStructure, step: ColonyState) -> u32 {
        ColonyState::iter().skip_while(|s| *s < step).map(|step| {
            let placed_count = self.num_placed_by(structure.structure_type(), step.clone());
            let max_count = structure.structure_type().controller_structures(step.controller_level() as u32);
            max_count.saturating_sub(placed_count as u32)
        }).min().unwrap()
    }

    pub fn is_free_at(&self, pos: RoomXY) -> bool {
        self.terrain.get(pos.x.u8(), pos.y.u8()) != Terrain::Wall && !self.pos2structure.contains_key(&pos)
    }

    pub fn num_placed_by(&self, ty: StructureType, step: ColonyState) -> u32 {
        self.structure_type_steps.get(&ty)
            .map(|x| 
                x.iter()
                    .take_while(|(place_step, _)| **place_step <= step)
                    .map(|(_, count)| *count)
                    .sum::<u32>()
            ).unwrap_or(0)
    }

    fn update_tile_pathing(&mut self, xy: RoomXY, ty: TilePathing) {
        self.cost_matrix.set_xy(xy, ty.cost());
    }

    pub fn plan_road(&mut self, xy: RoomXY, step: ColonyState) -> Result<(), String> {
        if self.roads.get(&xy).map_or(false, |old_step| step >= *old_step) { return Ok(()) }

        self.roads.insert(xy, step);
        self.update_tile_pathing(xy, TilePathing::PlannedRoad);

        Ok(())
    }

    pub fn plan_structure_earliest(&mut self, xy: RoomXY, structure: PlannedStructure) -> Result<ColonyState, String> {
        for step in ColonyState::iter() {
            if self.count_left_for(structure, step.clone()) == 0 { continue; };

            return self.plan_structure(xy, step.clone(), structure).map(|_| step)
        }

        Err(format!("Unable to build any more {structure:?}"))
    }

    pub fn plan_structure(&mut self, xy: RoomXY, step: ColonyState, structure: PlannedStructure) -> Result<(), String> {
        if self.terrain.get_xy(xy) == Terrain::Wall { return Err(format!("Can't plan {structure:?} due to wall")) };
        if self.num_placed_by(structure.structure_type(), step.clone()) >= structure.structure_type().controller_structures(step.controller_level().into()) { return Err(format!("Can't plan {structure:?} due to insufficient number of buildings at {step:?}")); }
        if self.pos2structure.get(&xy).map_or(false, |other| structure != *other) { return Err(format!("Can't plan {structure:?} due to overlap")) };
        if self.structures.get(&xy).map_or(false, |old_step| step >= *old_step) { return Ok(()) }

        self.structures2pos.entry(structure).or_default().insert(xy);
        self.pos2structure.insert(xy, structure);
        self.structures.insert(xy, step.clone());
        *self.structure_type_steps.entry(structure.structure_type()).or_default().entry(step).or_default() += 1;

        if !structure.walkable() {
            self.update_tile_pathing(xy, TilePathing::Impassable);
        }

        Ok(())
    }

    pub fn find_path_between(&self, point1: RoomXY, point2: RoomXY, step: ColonyState) -> Vec<Step> {
        let mut cost_matrix = self.cost_matrix.clone();
        for pos in self.roads.iter().filter(|(_, road_step)| **road_step <= step).map(|(pos, _)| pos) {
            cost_matrix.set_xy(*pos, TilePathing::BuiltRoad.cost());
        }
        
        let options = FindPathOptions::<fn(_, CostMatrix) -> SingleRoomCostResult, SingleRoomCostResult>::default()
            .cost_callback(|_, _| SingleRoomCostResult::CostMatrix(cost_matrix.clone()));

        let point1 = Position::new(point1.x, point1.y, self.room.name());
        let point2 = Position::new(point2.x, point2.y, self.room.name());

        let path = point1.find_path_to(&point2, Some(options));

        let Path::Vectorized(path) = path else { unreachable!() };
        path
    }

    pub fn plan_road_between(&mut self, point1: RoomXY, point2: RoomXY, step: ColonyState) -> Result<(), String> {
        let path = self.find_path_between(point1, point2, step.clone());

        let mut pos = Position::new(point1.x, point1.y, self.room.name());
        for path_step in path.iter() {
            pos.offset(path_step.dx, path_step.dy);

            if self.pos2structure.get(&pos.xy()).map_or(true, |structure| structure.walkable()) && self.terrain.get(pos.x().u8(), pos.y().u8()) != Terrain::Wall {
                self.plan_road(pos.xy(), step.clone())?;
            }
        }

        Ok(())
    }
}

struct CenterPlanner {
    flood_fill: FloodFill<DiagonalWalkableNeighs>,
    roads_utility_increases: HashMap<RoomXY, Vec<ColonyState>>
}

impl CenterPlanner {
    pub fn new(planner: &ColonyPlanner, center: RoomXY) -> Self {
        Self { 
            flood_fill: FloodFill::new(vec![center], DiagonalWalkableNeighs(planner.room.get_terrain())), 
            roads_utility_increases: HashMap::new()
        }
    }

    pub fn next_structure_pos(&mut self, planner: &ColonyPlanner, step: ColonyState) -> Result<RoomXY, String> {        
        while let Some((_, pos)) = self.flood_fill.next() {
            if planner.pos2structure.get(&pos).is_some() { continue; }

            let road_neighs: Vec<_> = Direction::iter()
                .filter(|dir| dir.is_orthogonal())
                .flat_map(|dir| pos.checked_add_direction(*dir))
                .filter(|neigh| planner.terrain.get(neigh.x.u8(), neigh.y.u8()) != Terrain::Wall)
                .collect();

            for road_neigh in road_neighs {
                let road_utility_increases = self.roads_utility_increases.entry(road_neigh).or_default();
                road_utility_increases.push(step.clone());
            }

            return Ok(pos); 
        }

        Err(format!("No more positions in center"))
    }

    pub fn plan_structure(&mut self, planner: &mut ColonyPlanner, step: ColonyState, structure: PlannedStructure) -> Result<(), String> {
        let pos = self.next_structure_pos(planner, step.clone())?;
        planner.plan_structure(pos, step, structure)
    }

    pub fn plan_roads(self, planner: &mut ColonyPlanner) -> Result<(), String> {
        for (road_pos, increases) in self.roads_utility_increases.into_iter() {
            let Some(plan_step) = increases.into_iter().sorted().skip(2).next() else { continue; };
            planner.plan_road(road_pos, plan_step)?;
        }

        Ok(())
    }
}

struct WalkableNeighs(RoomTerrain);
impl Neigh for WalkableNeighs {
    fn neighbors_of(&self, pos: RoomXY) -> impl Iterator<Item = RoomXY> {
        Direction::iter()
            .flat_map(move |dir| pos.checked_add_direction(*dir))
            .filter(|neigh| self.0.get(neigh.x.u8(), neigh.y.u8()) != Terrain::Wall)
    }
}

struct DiagonalWalkableNeighs(RoomTerrain);
impl Neigh for DiagonalWalkableNeighs {
    fn neighbors_of(&self, pos: RoomXY) -> impl Iterator<Item = RoomXY> {
        Direction::iter()
            .filter(|dir| dir.is_diagonal())
            .flat_map(move |dir| pos.checked_add_direction(*dir))
            .filter(|neigh| self.0.get(neigh.x.u8(), neigh.y.u8()) != Terrain::Wall)
    }
}

struct OrthogonalWalkableNeighs(RoomTerrain);
impl Neigh for OrthogonalWalkableNeighs {
    fn neighbors_of(&self, pos: RoomXY) -> impl Iterator<Item = RoomXY> {
        Direction::iter()
            .filter(|dir| dir.is_orthogonal())
            .flat_map(move |dir| pos.checked_add_direction(*dir))
            .filter(|neigh| self.0.get(neigh.x.u8(), neigh.y.u8()) != Terrain::Wall)
    }
}

trait Neigh {
    fn neighbors_of(&self, pos: RoomXY) -> impl Iterator<Item = RoomXY>;
}

struct FloodFill<N: Neigh> {
    queue: VecDeque<(usize, RoomXY)>,
    filled: HashSet<RoomXY>,

    neighs: N
}

impl<N> FloodFill<N> where N: Neigh {
    fn new<T>(seed: T, neighs: N) -> Self where T : IntoIterator<Item = RoomXY> {
        let mut queue = VecDeque::new();
        let mut filled = HashSet::new();

        for pos in seed {
            filled.insert(pos);
            queue.push_back((0, pos));
        }

        Self { queue, filled, neighs }
    }
}

impl<N> Iterator for FloodFill<N> where N: Neigh {
    type Item = (usize, RoomXY);

    fn next(&mut self) -> Option<Self::Item> {
        let (dist, pos) = self.queue.pop_front()?;
        let neighs = self.neighs.neighbors_of(pos);

        for new_neigh in neighs {
            if !self.filled.insert(new_neigh) { continue; }
            self.queue.push_back((dist + 1, new_neigh));
        }

        Some((dist, pos))
    }
}

fn draw_roads(visuals: &RoomVisual, roads: &HashSet<RoomXY>) {
    debug!("Roads: {}", roads.len());

    let connections: HashSet<_> = roads.iter()
        .flat_map(|road| 
            road.neighbors().into_iter()
                .filter(|neigh| roads.contains(neigh))
                .map(|neigh| 
                    vec![*road, neigh].into_iter()
                        .sorted()
                        .collect_tuple::<(_, _)>()
                        .unwrap()
                )
        ).collect();

    debug!("Connections: {}", connections.len());

    for (a, b) in connections.into_iter() {
        let a = (a.x.u8() as f32, a.y.u8() as f32);
        let b = (b.x.u8() as f32, b.y.u8() as f32);

        visuals.line(a, b, Some(LineStyle::default().opacity(0.75).width(0.2).color("#335882")));
    }
}

impl ColonyPlan {
    fn draw_until(&self, visuals: &RoomVisual, stop_step: Option<ColonyState>) {
        let mut roads = HashSet::new();

        for step in ColonyState::iter() {
            if stop_step.as_ref().map_or(false, |stop_step| step > *stop_step) { break; }
            let Some(step) = self.steps.get(&step) else { continue; };

            for (pos, structure) in &step.new_structures {
                structure.draw(visuals, pos);
            }

            roads.extend(step.new_roads.iter().cloned());
        }

        draw_roads(visuals, &roads);
    }

    fn draw_progression(&self, room: RoomName) {
        let plan = self.clone();

        let mut step = ColonyState::default();
        draw_in_room(room, move |visuals| {
            plan.draw_until(visuals, Some(step.clone()));
            step = step.get_promotion().unwrap_or_default()
        });
    }
}

impl PlannedStructure {
    fn draw(&self, visuals: &RoomVisual, pos: &RoomXY) {
        match self {
            PlannedStructure::Extension => {
                visuals.circle(pos.x.u8() as f32, pos.y.u8() as f32, Some(CircleStyle::default().radius(0.3).opacity(0.75).fill("#b05836")));
            },
            _ => {
                visuals.circle(pos.x.u8() as f32, pos.y.u8() as f32, Some(CircleStyle::default().radius(0.45).opacity(0.75).fill("#b05836")));

                let label = self.structure_type().to_string();
                visuals.text(pos.x.u8() as f32, pos.y.u8() as f32, label, Some(TextStyle::default().custom_font("0.35 Consolas").opacity(0.75).align(screeps::TextAlign::Center)));
            }
        }
    }
}