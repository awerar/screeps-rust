use std::collections::{BTreeMap, HashMap, HashSet};

use itertools::Itertools;
use screeps::{CostMatrix, CostMatrixSet, Direction, FindPathOptions, HasId, ObjectId, Path, Position, Room, RoomTerrain, RoomXY, Source, Step, StructureType, Terrain, find, pathfinder::SingleRoomCostResult};
use serde::{Deserialize, Serialize};

use crate::colony::{planning::{floodfill::{DiagonalWalkableNeighs, FloodFill}, plan::{CenterPlan, ColonyPlan, ColonyPlanStep, MineralPlan, SourcePlan, SourcesPlan}, planned_ref::PlannedStructureRef}, steps::ColonyStep};

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
    MineralContainer,
    Observer,
}

impl PlannedStructure {
    fn walkable(&self) -> bool {
        use PlannedStructure::*;

        matches!(self, SourceContainer(_) | ContainerStorage | SourceSpawn(_))
    }

    fn buildable_on_wall(&self) -> bool {
        use PlannedStructure::*;

        matches!(self, Extractor)
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
            PlannedStructure::MineralContainer => Container,
            PlannedStructure::Observer => Observer,
        }
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

    pub roads: HashMap<RoomXY, ColonyStep>,
    pub structures: HashMap<RoomXY, ColonyStep>,

    pub pos2structure: HashMap<RoomXY, PlannedStructure>,
    pub structures2pos: HashMap<PlannedStructure, HashSet<RoomXY>>,
    pub structure_type_steps: HashMap<StructureType, BTreeMap<ColonyStep, u32>>
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

        for step in ColonyStep::iter() {
            let mut plan_step = ColonyPlanStep::default();
            
            plan_step.new_roads.extend(self.roads.iter()
                .filter(|(_, road_step)| **road_step == step)
                .map(|(pos, _)| *pos)
            );

            plan_step.new_structures.extend(self.structures.iter()
                .filter(|(_, road_step)| **road_step == step)
                .map(|(pos, _)| (*pos, self.pos2structure[pos].structure_type()))
            );

            plan_steps.insert(step, plan_step);
        }

        Ok(ColonyPlan { 
            steps: plan_steps,
            center: self.compile_center()?,
            sources: self.compile_sources()?,
            mineral: self.compile_mineral()?
        })
    }

    fn compile_center(&self) -> Result<CenterPlan, String> {
        use PlannedStructure::*;

        let storage_ref = self.get_structure_ref(Storage)?;

        Ok(CenterPlan {
            pos: storage_ref.pos,
            spawn: self.get_structure_ref(MainSpawn)?, 
            storage: storage_ref,
            container_storage: self.get_structure_ref(ContainerStorage)?, 
            link: self.get_structure_ref(CentralLink)?, 
            terminal: self.get_structure_ref(Terminal)?, 
            observer: self.get_structure_ref(Observer)?, 
            towers: self.get_structure_refs(Tower)?, 
            extensions: self.get_structure_refs(Extension)?
        })
    }

    fn compile_sources(&self) -> Result<SourcesPlan, String> {
        use PlannedStructure::*;

        Ok(SourcesPlan(
            self.room.find(find::SOURCES, None).into_iter()
            .map(|source| source.id())
            .map(|source| {
                let plan = SourcePlan {
                    spawn: self.get_structure_ref(SourceSpawn(source))?,
                    container: self.get_structure_ref(SourceContainer(source))?,
                    link: self.get_structure_ref(SourceLink(source))?,
                    extensions: self.get_structure_refs(SourceExtension(source))?,
                };

                Ok((source, plan))
            }).collect::<Result<_, String>>()?
        ))
    }

    fn compile_mineral(&self) -> Result<MineralPlan, String> {
        use PlannedStructure::*;

        Ok(MineralPlan { 
            container: self.get_structure_ref(MineralContainer)?,
            extractor: self.get_structure_ref(Extractor)?, 
        })
    }

    pub fn get_structure_ref<T>(&self, structure: PlannedStructure) -> Result<PlannedStructureRef<T>, String> {
        self.structures2pos.get(&structure)
            .ok_or(format!("No {:?} was found", structure))
            .and_then(|positions| {
                if positions.len() == 0 { Err(format!("No {:?} was found", structure)) }
                else if positions.len() > 1 { Err(format!("Unable to determine unique {:?}", structure)) }
                else { Ok(positions.iter().next().unwrap().clone()) } 
            })
            .map(|pos| PlannedStructureRef::new(pos, &self.room))
    }

    pub fn get_structure_refs<T>(&self, structure: PlannedStructure) -> Result<Vec<PlannedStructureRef<T>>, String> {
        let Some(positions) = self.structures2pos.get(&structure) else { return Ok(Vec::new()) };

        Ok(positions.iter()
            .cloned()
            .map(|pos| PlannedStructureRef::new(pos, &self.room))
            .collect())
    }

    pub fn count_left_for(&self, structure: PlannedStructure, step: ColonyStep) -> u32 {
        ColonyStep::iter().skip_while(|s| *s < step).map(|step| {
            let placed_count = self.num_placed_by(structure.structure_type(), step);
            let max_count = structure.structure_type().controller_structures(step.controller_level() as u32);
            max_count.saturating_sub(placed_count as u32)
        }).min().unwrap()
    }

    pub fn is_free_at(&self, pos: RoomXY) -> bool {
        self.terrain.get(pos.x.u8(), pos.y.u8()) != Terrain::Wall && !self.pos2structure.contains_key(&pos)
    }

    pub fn num_placed_by(&self, ty: StructureType, step: ColonyStep) -> u32 {
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

    pub fn plan_road(&mut self, xy: RoomXY, step: ColonyStep) -> Result<(), String> {
        if self.roads.get(&xy).map_or(false, |old_step| step >= *old_step) { return Ok(()) }

        self.roads.insert(xy, step);
        self.update_tile_pathing(xy, TilePathing::PlannedRoad);

        Ok(())
    }

    pub fn plan_structure_earliest(&mut self, xy: RoomXY, structure: PlannedStructure) -> Result<ColonyStep, String> {
        let build_steps = &*self.structure_type_steps.entry(structure.structure_type()).or_default();
        let built_by = ColonyStep::iter()
            .map(|step| (step, build_steps.get(&step).cloned().unwrap_or(0)))
            .fold((0, HashMap::new()), |(mut count_acc, mut map_acc), (step, count)| {
                count_acc += count;
                map_acc.insert(step, count_acc);
                (count_acc, map_acc)
            }).1;

        let step = ColonyStep::iter()
            .collect_vec()
            .into_iter()
            .rev()
            .take_while(|step| built_by[step] < structure.structure_type().controller_structures(step.controller_level() as u32))
            .last().ok_or(format!("Unable to build any more {structure:?}"))?;

        self.plan_structure(xy, step, structure).map(|_| step)
    }

    pub fn plan_structure(&mut self, xy: RoomXY, step: ColonyStep, structure: PlannedStructure) -> Result<(), String> {
        if !structure.buildable_on_wall() && self.terrain.get_xy(xy) == Terrain::Wall { return Err(format!("Can't plan {structure:?} due to wall")) };
        if self.num_placed_by(structure.structure_type(), step) >= structure.structure_type().controller_structures(step.controller_level().into()) { return Err(format!("Can't plan {structure:?} due to insufficient number of buildings at {step:?}")); }
        if self.pos2structure.get(&xy).map_or(false, |other| structure != *other) { return Err(format!("Can't plan {structure:?} due to overlap")) };
        if self.structures.get(&xy).map_or(false, |old_step| step >= *old_step) { return Ok(()) }

        self.structures2pos.entry(structure).or_default().insert(xy);
        self.pos2structure.insert(xy, structure);
        self.structures.insert(xy, step);
        *self.structure_type_steps.entry(structure.structure_type()).or_default().entry(step).or_default() += 1;

        if !structure.walkable() {
            self.update_tile_pathing(xy, TilePathing::Impassable);
        }

        Ok(())
    }

    pub fn find_path_between(&self, point1: RoomXY, point2: RoomXY, step: ColonyStep) -> Vec<Step> {
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

    pub fn plan_road_between(&mut self, point1: RoomXY, point2: RoomXY, step: ColonyStep) -> Result<(), String> {
        let path = self.find_path_between(point1, point2, step);

        let mut pos = Position::new(point1.x, point1.y, self.room.name());
        for path_step in path.iter() {
            pos.offset(path_step.dx, path_step.dy);

            if self.pos2structure.get(&pos.xy()).map_or(true, |structure| structure.walkable()) && self.terrain.get(pos.x().u8(), pos.y().u8()) != Terrain::Wall {
                self.plan_road(pos.xy(), step)?;
            }
        }

        Ok(())
    }
}

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

    pub fn next_structure_pos(&mut self, planner: &ColonyPlanner, step: ColonyStep) -> Result<RoomXY, String> {        
        while let Some((_, pos)) = self.flood_fill.next() {
            if planner.pos2structure.get(&pos).is_some() { continue; }

            let road_neighs: Vec<_> = Direction::iter()
                .filter(|dir| dir.is_orthogonal())
                .flat_map(|dir| pos.checked_add_direction(*dir))
                .filter(|neigh| planner.terrain.get(neigh.x.u8(), neigh.y.u8()) != Terrain::Wall)
                .collect();

            for road_neigh in road_neighs {
                let road_utility_increases = self.roads_utility_increases.entry(road_neigh).or_default();
                road_utility_increases.push(step);
            }

            return Ok(pos); 
        }

        Err(format!("No more positions in center"))
    }

    pub fn plan_structure(&mut self, planner: &mut ColonyPlanner, step: ColonyStep, structure: PlannedStructure) -> Result<(), String> {
        let pos = self.next_structure_pos(planner, step)?;
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