use std::{assert_matches, collections::{HashMap, HashSet}};

use itertools::Itertools;
use screeps::{CostMatrix, CostMatrixSet, FindPathOptions, HasPosition, Path, Position, Room, RoomTerrain, RoomXY, Source, Step, StructureType, Terrain, find, pathfinder::SingleRoomCostResult};
use serde::{Deserialize, Serialize};
use anyhow::anyhow;
use strum::IntoEnumIterator;

use crate::{colony::{plan::{CenterPlan, ColonyPlan, ColonyPlanStep, MineralPlan, SourcePlan, SourcesPlan, refs::{PlannedStructureRef, PlannedStructureRefs}}, steps::ColonyStep}, domain_traits::HasId};

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Clone, Copy, Debug)]
pub enum PlannedStructure {
    MainSpawn,
    SourceSpawn(screeps::ObjectId<Source>),
    SourceContainer(screeps::ObjectId<Source>),
    SourceLink(screeps::ObjectId<Source>),
    SourceExtension(screeps::ObjectId<Source>),
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

        matches!(self, SourceContainer(_) | ContainerStorage | MineralContainer)
    }

    fn buildable_on_wall(&self) -> bool {
        use PlannedStructure::*;

        matches!(self, Extractor)
    }

    fn structure_type(&self) -> StructureType {
        use StructureType::*;

        match self {
            PlannedStructure::MainSpawn
            | PlannedStructure::SourceSpawn(_) => Spawn,
            PlannedStructure::SourceContainer(_)
            | PlannedStructure::ContainerStorage
            | PlannedStructure::MineralContainer => Container,
            PlannedStructure::Extension
            | PlannedStructure::SourceExtension(_) => Extension,
            PlannedStructure::CentralLink
            | PlannedStructure::SourceLink(_) => Link,
            PlannedStructure::Storage => Storage,
            PlannedStructure::Tower => Tower,
            PlannedStructure::Terminal => Terminal,
            PlannedStructure::Extractor => Extractor,
            PlannedStructure::Observer => Observer,
        }
    }
}

pub enum PlannedCreep {
    Upgrader
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
    fn cost(self) -> u8 {
        match self {
            TilePathing::BuiltRoad => 5,
            TilePathing::PlannedRoad => 6,
            TilePathing::Plains => 8,
            TilePathing::Swamp => 20,
            TilePathing::Impassable => 255,
        }
    }
}

enum PlannedEntity {
    Structure(PlannedStructure),
    Creep(PlannedCreep)
}

impl PlannedEntity {
    fn structure_type(&self) -> Option<StructureType> {
        match self {
            PlannedEntity::Structure(structure) => Some(structure.structure_type()),
            PlannedEntity::Creep(_) => None,
        }
    }
}

// Impassable structures are not allowed lifetimes
struct LifeTime {
    pub build_step: ColonyStep,
    pub destroy_step: Option<ColonyStep>
}

impl LifeTime {
    fn is_built_at(&self, step: ColonyStep) -> bool {
        step >= self.build_step && self.destroy_step.is_none_or(|destroy_step| destroy_step > step)
    }

    fn is_planned_at(&self, step: ColonyStep) -> bool {
        step < self.build_step
    }

    fn is_destroyed_at(&self, step: ColonyStep) -> bool {
        self.destroy_step.is_some_and(|destroy_step| step > destroy_step)
    }

    fn union(&self, other: &Self) -> Self {
        Self { 
            build_step: self.build_step.min(other.build_step), 
            destroy_step: match (self.destroy_step, other.destroy_step) {
                (Some(step1), Some(step2)) => Some(step1.max(step2)),
                (step1, step2) => step1.or(step2)
            }
        }
    }
}

pub struct IntermediateColonyPlan {
    pub room: Room,

    pub new_structures: HashMap<ColonyStep, HashSet<(RoomXY, StructureType)>>,
    pub expired_structures: HashMap<ColonyStep, HashSet<(RoomXY, StructureType)>>,

    pub roads: RoadPlanner
}

impl IntermediateColonyPlan {
    fn compile(self) -> anyhow::Result<ColonyPlan> {
        let mut plan_steps = HashMap::new();

        let accumulated_structures = HashSet::new();
        for step in ColonyStep::iter() {
            let new_structures = self.new_structures.get(&step).unwrap();
            let expired_structures = self.expired_structures.get(&step).unwrap();

            accumulated_structures.extend(new_structures.iter().cloned());
            accumulated_structures = accumulated_structures.difference(expired_structures).cloned().collect();

            plan_steps.insert(step, ColonyPlanStep { 
                new_structures: new_structures.clone(), 
                expired_structures: expired_structures.clone(), 
                accumulated_structures: accumulated_structures.clone()
            });
        }

        let center = self.compile_center()?;

        Ok(ColonyPlan {
            steps: plan_steps,
            sources: self.compile_sources(center.pos.xy())?,
            mineral: self.compile_mineral(center.pos.xy())?,
            center,
            controller: PlannedStructureRef::new(self.room.controller().ok_or(anyhow!("No controller"))?.pos())
        })
    }


    fn compile_center(&self) -> anyhow::Result<CenterPlan> {
        use PlannedStructure::*;

        let storage_ref = self.get_structure_ref(Storage)?;

        Ok(CenterPlan {
            pos: storage_ref.pos,
            spawn: self.get_structure_ref(MainSpawn)?,
            storage: storage_ref.into(),
            container_storage: self.get_structure_ref(ContainerStorage)?.into(),
            link: self.get_structure_ref(CentralLink)?.into(),
            terminal: self.get_structure_ref(Terminal)?.into(),
            observer: self.get_structure_ref(Observer)?.into(),
            towers: self.get_structure_refs(Tower),
            extensions: self.get_structure_refs(Extension)
        })
    }

    fn compile_sources(&self, center: RoomXY) -> anyhow::Result<SourcesPlan> {
        use PlannedStructure::*;

        self.room.find(find::SOURCES, None).into_iter()
            .map(|source| screeps::HasId::id(&source))
            .map(|source| {
                let container = self.get_structure_ref(SourceContainer(source))?;
                let spawn = self.get_structure_ref(SourceSpawn(source))?;

                let plan = SourcePlan {
                    spawn_direction: spawn.pos.get_direction_to(container.pos).unwrap(),
                    distance: self.roads.find_path_between(center, container.pos.xy(), None).len() as u32,
                    spawn: spawn.into(),
                    road: PlannedStructureRef::new(container.pos),
                    container: container.into(),
                    link: self.get_structure_ref(SourceLink(source))?.into(),
                    extensions: self.get_structure_refs(SourceExtension(source)),
                };

                Ok((source, plan))
            }).collect::<anyhow::Result<_>>()
    }

    fn compile_mineral(&self, center: RoomXY) -> anyhow::Result<MineralPlan> {
        use PlannedStructure::*;

        let container = self.get_structure_ref(MineralContainer)?;

        Ok(MineralPlan {
            distance: self.roads.find_path_between(center, container.pos.xy(), None).len() as u32,
            container: container.into(),
            extractor: self.get_structure_ref(Extractor)?.into(),
        })
    }

    fn get_structure_ref<T: HasId>(&self, structure: PlannedStructure) -> anyhow::Result<PlannedStructureRef<T>> {
        self.structures2pos.get(&structure)
            .ok_or(anyhow!("No {structure:?} was found"))
            .and_then(|positions| {
                if positions.is_empty() { Err(anyhow!("No {structure:?} was found")) }
                else if positions.len() > 1 { Err(anyhow!("Unable to determine unique {structure:?}")) }
                else { Ok(*positions.iter().next().unwrap()) }
            })
            .map(|pos| PlannedStructureRef::new(Position::new(pos.x, pos.y, self.room.name())))
    }

    fn get_structure_refs<T: HasId>(&self, structure: PlannedStructure) -> PlannedStructureRefs<T> {
        let Some(positions) = self.structures2pos.get(&structure) else { return PlannedStructureRefs(Vec::new()) };

        PlannedStructureRefs(positions.iter()
            .copied()
            .map(|pos| PlannedStructureRef::new(Position::new(pos.x, pos.y, self.room.name())))
            .collect())
    }
}

pub struct ColonyPlanner {
    pub room: Room,
    pub terrain: RoomTerrain,

    pub roads: RoadPlanner,
    pub entities: HashMap<RoomXY, Vec<(PlannedEntity, LifeTime)>>,
}

impl ColonyPlanner {
    pub fn new(room: Room) -> Self {
        ColonyPlanner {
            roads: RoadPlanner::new(room.clone()),
            terrain: room.get_terrain(),
            room,
            entities: HashMap::new(),
        }
    }

    fn compile_intermediate(self) -> anyhow::Result<IntermediateColonyPlan> {

    }

    pub fn compile(self) -> anyhow::Result<ColonyPlan> {
        self.compile_intermediate()?.compile()
    }

    pub fn count_left_for(&self, structure: PlannedStructure, step: ColonyStep) -> u32 {
        ColonyStep::iter().skip_while(|s| *s < step).map(|step| {
            let placed_count = self.num_placed_by(structure.structure_type(), step);
            let max_count = structure.structure_type().controller_structures(u32::from(step.controller_level()));
            max_count.saturating_sub(placed_count)
        }).min().unwrap()
    }

    pub fn is_free_at(&self, pos: RoomXY) -> bool {
        self.terrain.get(pos.x.u8(), pos.y.u8()) != Terrain::Wall && !self.pos2structure.contains_key(&pos)
    }

    pub fn num_placed_by(&self, ty: StructureType, step: ColonyStep) -> u32 {
        self.structure_type_steps.get(&ty)
            .map_or(0, |x|
                x.iter()
                    .take_while(|(place_step, _)| **place_step <= step)
                    .map(|(_, count)| *count)
                    .sum::<u32>()
            )
    }

    fn update_tile_pathing(&mut self, xy: RoomXY, ty: TilePathing) {
        self.cost_matrix.set_xy(xy, ty.cost());
    }

    pub fn plan_road(&mut self, xy: RoomXY, step: ColonyStep) {
        if self.roads.get(&xy).is_some_and(|old_step| step >= *old_step) { return; }

        self.roads.insert(xy, step);

        if !self.pos2structure.contains_key(&xy) {
            self.update_tile_pathing(xy, TilePathing::PlannedRoad);
        }
    }

    pub fn plan_structure_earliest(&mut self, xy: RoomXY, structure: PlannedStructure) -> anyhow::Result<ColonyStep> {
        let build_steps = &*self.structure_type_steps.entry(structure.structure_type()).or_default();
        let built_by = ColonyStep::iter()
            .map(|step| (step, build_steps.get(&step).copied().unwrap_or(0)))
            .fold((0, HashMap::new()), |(mut count_acc, mut map_acc), (step, count)| {
                count_acc += count;
                map_acc.insert(step, count_acc);
                (count_acc, map_acc)
            }).1;

        let step = ColonyStep::iter()
            .collect_vec()
            .into_iter()
            .rev()
            .take_while(|step| built_by[step] < structure.structure_type().controller_structures(u32::from(step.controller_level())))
            .last().ok_or(anyhow!("Unable to build any more {structure:?}"))?;

        self.plan_structure(xy, step, structure).map(|()| step)
    }

    pub fn plan_structure(&mut self, xy: RoomXY, step: ColonyStep, structure: PlannedStructure) -> anyhow::Result<()> {
        if !structure.buildable_on_wall() && self.terrain.get_xy(xy) == Terrain::Wall { return Err(anyhow!("Can't plan {structure:?} due to wall")) }
        if self.num_placed_by(structure.structure_type(), step) >= structure.structure_type().controller_structures(step.controller_level().into()) { return Err(anyhow!("Can't plan {structure:?} due to insufficient number of buildings at {step:?}")); }
        if self.pos2structure.get(&xy).is_some_and(|other| structure != *other) { return Err(anyhow!("Can't plan {structure:?} due to overlap")) }
        if self.entities.get(&xy).is_some_and(|old_step| step >= *old_step) { return Ok(()) }

        self.structures2pos.entry(structure).or_default().insert(xy);
        self.pos2structure.insert(xy, structure);
        self.entities.insert(xy, step);
        *self.structure_type_steps.entry(structure.structure_type()).or_default().entry(step).or_default() += 1;

        if !structure.walkable() {
            self.update_tile_pathing(xy, TilePathing::Impassable);
        }

        Ok(())
    }

    pub fn plan_road_between(&mut self, point1: RoomXY, point2: RoomXY, step: ColonyStep) {
        let path = self.find_path_between(point1, point2, Some(step));

        let mut pos = Position::new(point1.x, point1.y, self.room.name());
        for path_step in &path {
            pos.offset(path_step.dx, path_step.dy);

            if self.pos2structure.get(&pos.xy()).is_none_or(PlannedStructure::walkable) && self.terrain.get(pos.x().u8(), pos.y().u8()) != Terrain::Wall {
                self.plan_road(pos.xy(), step);
            }
        }
    }
}

struct RoadPlanner {
    room: Room,
    terrain: RoomTerrain,
    cost_matrix: CostMatrix,

    // We never rebuild a road, instead just extend the lifetime
    roads: HashMap<RoomXY, LifeTime>,

    // Once a blocker gets placed it will never disappear
    blocks: HashMap<RoomXY, ColonyStep>
}

impl RoadPlanner {
    pub fn new(room: Room) -> Self {
        let terrain = room.get_terrain();
        let cost_matrix = CostMatrix::new();
        for x in 0..50 {
            for y in 0..50 {
                cost_matrix.set(x, y, TilePathing::from(terrain.get(x, y)).cost());
            }
        }

        Self { 
            terrain, 
            cost_matrix, 
            room,
            roads: HashMap::new(),
            blocks: HashMap::new()
        }
    }

    // TODO: Split into road planning and distance checking
    pub fn find_path_between(&self, point1: RoomXY, point2: RoomXY, build_step: Option<ColonyStep>, expiration_step: Option<ColonyStep>) -> Vec<Step> {
        let step = build_step.unwrap_or(ColonyStep::last());
        let expiration_step = expiration_step.unwrap_or(ColonyStep::last());

        let mut cost_matrix = self.cost_matrix.clone();

        for (pos, lt) in &self.roads {
            if lt.is_built_at(step) {
                cost_matrix.set_xy(*pos, TilePathing::BuiltRoad.cost());
            } else if lt.is_planned_at(step) {
                cost_matrix.set_xy(*pos, TilePathing::PlannedRoad.cost());
            }
        }

        for (pos, block_step) in &self.blocks {
            if *block_step < expiration_step {
                cost_matrix.set_xy(*pos, TilePathing::Impassable.cost());
            }
        }

        let options = FindPathOptions::<fn(_, CostMatrix) -> SingleRoomCostResult, SingleRoomCostResult>::default()
            .cost_callback(|_, _| SingleRoomCostResult::CostMatrix(cost_matrix.clone()));

        let point1 = Position::new(point1.x, point1.y, self.room.name());
        let point2 = Position::new(point2.x, point2.y, self.room.name());

        let path = point1.find_path_to(&point2, Some(options));

        let Path::Vectorized(path) = path else { unreachable!() };
        path
    }

    pub fn add_blocker(&mut self, pos: RoomXY, step: ColonyStep) {
        let prev = self.blocks.insert(pos, step);
        assert_matches!(prev, None);
    }

    pub fn plan_road(&mut self, pos: RoomXY, lt: LifeTime) {
        self.roads.entry(pos).and_modify(|lt2| *lt2 = lt2.union(&lt)).or_insert(lt);
    }
}