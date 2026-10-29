use std::{collections::{BTreeMap, HashMap, HashSet, hash_map}, mem, ops::Not};

use itertools::Itertools;
use option_entry::OptionEntry;
use screeps::{CostMatrix, CostMatrixSet, FindPathOptions, HasPosition, Path, Position, Room, RoomTerrain, RoomXY, Source, Step, StructureType, Terrain, find, pathfinder::SingleRoomCostResult};
use serde::{Deserialize, Serialize};
use anyhow::{anyhow, bail};
use strum::IntoEnumIterator;

use crate::{colony::{plan::{CenterPlan, ColonyPlan, ColonyPlanStep, MineralPlan, SourcePlan, SourcesPlan, refs::{PlannedStructureRef, PlannedStructureRefs}}, steps::ColonyStep}, domain_traits::HasId};

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Clone, Copy, Debug)]
pub enum PlannedPermanentStructure {
    MainSpawn,
    SourceSpawn(screeps::ObjectId<Source>),
    SourceContainer(screeps::ObjectId<Source>),
    SourceLink(screeps::ObjectId<Source>),
    SourceExtension(screeps::ObjectId<Source>),
    Extension,
    Storage,
    Terminal,
    Tower,
    CentralLink,
    Extractor,
    MineralContainer,
    Observer,
}

impl PlannedPermanentStructure {
    fn walkable(&self) -> bool {
        use PlannedPermanentStructure::*;

        matches!(self, SourceContainer(_) | MineralContainer)
    }

    fn buildable_on_wall(&self) -> bool {
        use PlannedPermanentStructure::*;

        matches!(self, Extractor)
    }

    fn structure_type(&self) -> StructureType {
        use StructureType::*;

        match self {
            PlannedPermanentStructure::MainSpawn
            | PlannedPermanentStructure::SourceSpawn(_) => Spawn,
            PlannedPermanentStructure::SourceContainer(_)
            | PlannedPermanentStructure::MineralContainer => Container,
            PlannedPermanentStructure::Extension
            | PlannedPermanentStructure::SourceExtension(_) => Extension,
            PlannedPermanentStructure::CentralLink
            | PlannedPermanentStructure::SourceLink(_) => Link,
            PlannedPermanentStructure::Storage => Storage,
            PlannedPermanentStructure::Tower => Tower,
            PlannedPermanentStructure::Terminal => Terminal,
            PlannedPermanentStructure::Extractor => Extractor,
            PlannedPermanentStructure::Observer => Observer,
        }
    }
}

// Has to be walkable
#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Clone, Copy, Debug)]
pub enum PlannedTemporaryStructure {
    ContainerBuffer
}

impl PlannedTemporaryStructure {
    fn structure_type(&self) -> StructureType {
        match self {
            PlannedTemporaryStructure::ContainerBuffer => StructureType::Container
        }
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Clone, Copy, Debug)]
pub enum PlannedStructure {
    Temporary(PlannedTemporaryStructure),
    Permanent(PlannedPermanentStructure)
}

impl From<PlannedPermanentStructure> for PlannedStructure {
    fn from(value: PlannedPermanentStructure) -> Self {
        Self::Permanent(value)
    }
}

impl From<PlannedTemporaryStructure> for PlannedStructure {
    fn from(value: PlannedTemporaryStructure) -> Self {
        Self::Temporary(value)
    }
}

impl PlannedStructure {
    fn structure_type(&self) -> StructureType {
        match self {
            PlannedStructure::Temporary(structure) => structure.structure_type(),
            PlannedStructure::Permanent(structure) => structure.structure_type(),
        }
    }
}

impl From<PlannedStructure> for PlannedEntity {
    fn from(value: PlannedStructure) -> Self {
        match value {
            PlannedStructure::Temporary(structure) => 
                PlannedEntity::TemporaryStructure(structure),
            PlannedStructure::Permanent(structure) => 
                PlannedEntity::Blocker(PlannedBlocker::Structure(structure)),
        }
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Clone, Copy, Debug)]
pub enum PlannedPermanentCreep {
    Upgrader
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Clone, Copy, Debug)]
pub enum PlannedBlocker {
    Creep(PlannedPermanentCreep),
    Structure(PlannedPermanentStructure)
}

impl PlannedBlocker {
    fn walkable(&self) -> bool {
        match self {
            PlannedBlocker::Creep(_) => false,
            PlannedBlocker::Structure(structure) => structure.walkable(),
        }
    }

    fn structure_type(&self) -> Option<StructureType> {
        match self {
            PlannedBlocker::Creep(_) => None,
            PlannedBlocker::Structure(structure) => Some(structure.structure_type()),
        }
    }

    fn plannable_on_wall(&self) -> bool {
        match self {
            PlannedBlocker::Creep(_) => false,
            PlannedBlocker::Structure(structure) => structure.buildable_on_wall(),
        }
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Clone, Copy, Debug)]
pub enum PlannedEntity {
    Blocker(PlannedBlocker),
    TemporaryStructure(PlannedTemporaryStructure)
}

impl From<PlannedBlocker> for PlannedEntity { fn from(value: PlannedBlocker) -> Self { Self::Blocker(value) }}
impl From<PlannedTemporaryStructure> for PlannedEntity { fn from(value: PlannedTemporaryStructure) -> Self { Self::TemporaryStructure(value) }}
impl From<PlannedPermanentStructure> for PlannedEntity { fn from(value: PlannedPermanentStructure) -> Self { Self::Blocker(PlannedBlocker::Structure(value)) }}
impl From<PlannedPermanentCreep> for PlannedEntity { fn from(value: PlannedPermanentCreep) -> Self { Self::Blocker(PlannedBlocker::Creep(value)) }}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum TileHistory {
    Temporary(LifeTime, PlannedTemporaryStructure, Option<Box<TileHistory>>),
    Blocker(ColonyStep, PlannedBlocker)
}

impl TileHistory {
    fn get_impassable_step(&self) -> Option<ColonyStep> {
        match self {
            TileHistory::Temporary(_, _, tile_history) => 
                tile_history.as_ref().and_then(|future| future.get_impassable_step()),
            TileHistory::Blocker(colony_step, blocker) => 
                blocker.walkable().not().then_some(*colony_step),
        }
    }

    fn get_earliest_extension_step(&self) -> Option<ColonyStep> {
        match self {
            TileHistory::Temporary(lt, _, future) => 
                future.as_ref().map_or(lt.destroy_step, |future| future.get_earliest_extension_step()),
            TileHistory::Blocker(_, _) => None,
        }
    }

    fn get_start_step(&self) -> ColonyStep {
        match self {
            TileHistory::Temporary(lt, _, _) => lt.build_step,
            TileHistory::Blocker(step, _) => *step,
        }
    }

    fn extend(&mut self, future: TileHistory) -> anyhow::Result<()> {
        match self {
            TileHistory::Temporary(lt, _, curr_future) => {
                match curr_future.entry() {
                    option_entry::Entry::Vacant(entry) => {
                        if !lt.is_destroyed_at(future.get_start_step()) { 
                            bail!("Can't extend history ending at {:?} with a history starting at {:?}", lt.destroy_step, future.get_start_step()) 
                        }

                        entry.insert(Box::new(future));
                        Ok(())
                    },
                    option_entry::Entry::Occupied(mut entry) => 
                        entry.get_mut().extend(future),
                }
            },
            TileHistory::Blocker(_, _) => bail!("Can't extend a blocked history"),
        }
    }

    fn add_temporary(&mut self, lt: LifeTime, structure: PlannedTemporaryStructure) -> anyhow::Result<()> {
        self.extend(TileHistory::Temporary(lt, structure, None))
    }

    fn block(&mut self, step: ColonyStep, blocker: PlannedBlocker) -> anyhow::Result<()> {
        self.extend(TileHistory::Blocker(step, blocker))
    }
}

pub struct TileHistoryIterator(Option<TileHistory>);

impl Iterator for TileHistoryIterator {
    type Item = (PlannedEntity, LifeTime);

    fn next(&mut self) -> Option<Self::Item> {
        match mem::replace(&mut self.0, None) {
            None => None,
            Some(TileHistory::Temporary(lt, structure, future)) => {
                self.0 = future.map(|x| *x);
                Some((PlannedEntity::TemporaryStructure(structure), lt))
            },
            Some(TileHistory::Blocker(step, blocker)) => {
                Some((PlannedEntity::Blocker(blocker), LifeTime::permanent(step)))
            },
        }
    }
}

impl IntoIterator for TileHistory {
    type Item = (PlannedEntity, LifeTime);
    type IntoIter = TileHistoryIterator;

    fn into_iter(self) -> Self::IntoIter {
        TileHistoryIterator(Some(self))
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

// Impassable structures are not allowed lifetimes
#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
struct LifeTime {
    pub build_step: ColonyStep,
    pub destroy_step: Option<ColonyStep>
}

impl LifeTime {
    fn permanent(build: ColonyStep) -> Self {
        LifeTime { build_step: build, destroy_step: None }
    }

    fn temporary(build: ColonyStep, destroy: ColonyStep) -> Option<Self> {
        if destroy <= build { return None }

        Some(LifeTime { build_step: build, destroy_step: Some(destroy) })
    }

    fn is_built_at(&self, step: ColonyStep) -> bool {
        step >= self.build_step && self.destroy_step.is_none_or(|destroy_step| destroy_step > step)
    }

    fn is_planned_at(&self, step: ColonyStep) -> bool {
        step < self.build_step
    }

    fn is_destroyed_at(&self, step: ColonyStep) -> bool {
        self.destroy_step.is_some_and(|destroy_step| step >= destroy_step)
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

    pub entities2pos: HashMap<PlannedEntity, Vec<RoomXY>>,
}

impl IntermediateColonyPlan {
    fn compile(self) -> anyhow::Result<ColonyPlan> {
        use PlannedPermanentCreep::*;

        let mut plan_steps = HashMap::new();

        let mut accumulated_structures = HashSet::new();
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
            upgrader_pos: self.get_pos(Upgrader)?,
            controller: PlannedStructureRef::new(self.room.controller().ok_or(anyhow!("No controller"))?.pos())
        })
    }


    fn compile_center(&self) -> anyhow::Result<CenterPlan> {
        use PlannedPermanentStructure::*;
        use PlannedTemporaryStructure::*;

        let storage_ref = self.get_structure_ref(Storage)?;

        Ok(CenterPlan {
            pos: storage_ref.pos,
            spawn: self.get_structure_ref(MainSpawn)?,
            storage: storage_ref.into(),
            container_storage: self.get_structure_ref(ContainerBuffer)?.into(),
            link: self.get_structure_ref(CentralLink)?.into(),
            terminal: self.get_structure_ref(Terminal)?.into(),
            observer: self.get_structure_ref(Observer)?.into(),
            towers: self.get_structure_refs(Tower),
            extensions: self.get_structure_refs(Extension)
        })
    }

    fn compile_sources(&self, center: RoomXY) -> anyhow::Result<SourcesPlan> {
        use PlannedPermanentStructure::*;

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
        use PlannedPermanentStructure::*;

        let container = self.get_structure_ref(MineralContainer)?;

        Ok(MineralPlan {
            distance: self.roads.find_path_between(center, container.pos.xy(), None).len() as u32,
            container: container.into(),
            extractor: self.get_structure_ref(Extractor)?.into(),
        })
    }

    fn get_pos<E : Into<PlannedEntity>>(&self, entity: E) -> anyhow::Result<Position> {
        let entity = entity.into();
        self.entities2pos.get(&entity)
            .ok_or(anyhow!("No {entity:?} was found"))
            .and_then(|positions| {
                if positions.is_empty() { Err(anyhow!("No {entity:?} was found")) }
                else if positions.len() > 1 { Err(anyhow!("Unable to determine unique {entity:?}")) }
                else { Ok(*positions.iter().next().unwrap()) }
            }).map(|pos| Position::new(pos.x, pos.y, self.room.name()))
    }

    fn get_structure_ref<T: HasId, S : Into<PlannedStructure>>(&self, structure: S) -> anyhow::Result<PlannedStructureRef<T>> {
        let structure = structure.into();
        self.get_pos(structure)
            .map(|pos| PlannedStructureRef::new(pos))
    }

    fn get_structure_refs<T: HasId, S : Into<PlannedStructure>>(&self, structure: S) -> PlannedStructureRefs<T> {
        let structure = structure.into();
        let Some(positions) = self.entities2pos.get(&structure.into()) else { return PlannedStructureRefs(Vec::new()) };

        PlannedStructureRefs(positions.iter()
            .copied()
            .map(|pos| PlannedStructureRef::new(Position::new(pos.x, pos.y, self.room.name())))
            .collect())
    }
}

pub struct ColonyPlanner {
    pub room: Room,
    pub terrain: RoomTerrain,

    pub base_cost_matrix: CostMatrix,

    // We never rebuild a road, instead just extend the lifetime
    pub roads: HashMap<RoomXY, LifeTime>,
    pub tiles: HashMap<RoomXY, TileHistory>,

    pub structure_steps: HashMap<StructureType, BTreeMap<ColonyStep, i32>>
}

impl ColonyPlanner {
    pub fn new(room: Room) -> Self {
        let terrain = room.get_terrain();
        let base_cost_matrix = CostMatrix::new();
        for x in 0..50 {
            for y in 0..50 {
                base_cost_matrix.set(x, y, TilePathing::from(terrain.get(x, y)).cost());
            }
        }

        ColonyPlanner {
            base_cost_matrix,
            terrain,
            room,
            structure_steps: HashMap::new(),
            roads: HashMap::new(),
            tiles: HashMap::new(),
        }
    }

    fn compile_intermediate(self) -> anyhow::Result<IntermediateColonyPlan> {

    }

    pub fn compile(self) -> anyhow::Result<ColonyPlan> {
        self.compile_intermediate()?.compile()
    }

    pub fn count_left_for(&self, structure: StructureType, step: ColonyStep) -> u32 {
        self.calculate_all_avaliable_at(structure).into_iter()
            .skip_while(|(s, _)| *s < step)
            .map(|(_, avaliable)| avaliable)
            .min().unwrap()
    }

    pub fn is_free_at(&self, pos: RoomXY) -> bool {
        self.terrain.get(pos.x.u8(), pos.y.u8()) != Terrain::Wall && !self.tiles.contains_key(&pos)
    }

    fn add_structure_lifetime(&mut self, structure: StructureType, lt: LifeTime) {
        let steps = self.structure_steps.entry(structure).or_default();
        *steps.entry(lt.build_step).or_default() += 1;
        if let Some(destroy_step) = lt.destroy_step {
            *steps.entry(destroy_step).or_default() -= 1;
        }
    }

    pub fn plan_road(&mut self, pos: RoomXY, lt: LifeTime) {
        self.roads.entry(pos).and_modify(|lt2| *lt2 = lt2.union(&lt)).or_insert(lt);
        self.add_structure_lifetime(StructureType::Road, lt);

    }

    fn calculate_all_avaliable_at(&self, structure: StructureType) -> BTreeMap<ColonyStep, u32> {
        let default_build_steps = BTreeMap::new();
        let build_steps = self.structure_steps.get(&structure).unwrap_or(&default_build_steps);

        ColonyStep::iter()
            .map(|step| (step, build_steps.get(&step).copied().unwrap_or(0)))
            .fold((0_u32, BTreeMap::new()), |(mut count_acc, mut map_acc), (step, count)| {
                count_acc = count_acc.checked_add_signed(count).unwrap();

                let max = structure.controller_structures(step.controller_level() as u32);
                let avaliable = max - count_acc;
                map_acc.insert(step,  avaliable);

                (count_acc, map_acc)
            }).1
    }

    pub fn plan_blocker_earliest(&mut self, xy: RoomXY, blocker: PlannedBlocker) -> anyhow::Result<ColonyStep> {
        let mut earliest = match self.tiles.get(&xy) {
            Some(history) => history.get_earliest_extension_step().ok_or_else(|| anyhow!("Can't extend history"))?,
            None => ColonyStep::default(),
        };

        if let Some(structure) = blocker.structure_type() {
            earliest = earliest.max(
                self.calculate_all_avaliable_at(structure).into_iter()
                    .rev()
                    .take_while(|(_, avaliable)| *avaliable > 0)
                    .last().ok_or(anyhow!("Unable to build any more {structure:?}"))?.0
            );
        }

        self.plan_blocker(xy, earliest, blocker).map(|()| earliest)
    }

    pub fn plan_blocker(&mut self, xy: RoomXY, step: ColonyStep, blocker: PlannedBlocker) -> anyhow::Result<()> {
        if !blocker.plannable_on_wall() && self.terrain.get_xy(xy) == Terrain::Wall { bail!("Can't plan {blocker:?} due to wall") }

        if let Some(structure) = blocker.structure_type() && self.count_left_for(structure, step) == 0 {
            bail!("Can't plan {structure:?} due to insufficient number of buildings at {step:?}");
        }

        match self.tiles.entry(xy) {
            hash_map::Entry::Occupied(mut entry) => entry.get_mut().block(step, blocker)?,
            hash_map::Entry::Vacant(entry) => { entry.insert(TileHistory::Blocker(step, blocker)); },
        }

        if let Some(structure) = blocker.structure_type() {
            self.add_structure_lifetime(structure, LifeTime::permanent(step));
        }

        Ok(())
    }

    pub fn plan_road_between(&mut self, point1: RoomXY, point2: RoomXY, lt: LifeTime) {
        let path = self.find_path_between(point1, point2, Some(lt.build_step), lt.destroy_step);

        for step in &path {
            let pos = RoomXY::try_from((step.x as u8, step.y as u8)).unwrap();
            self.plan_road(pos, lt);
        }
    }

    // TODO: Split into road planning and distance checking
    pub fn find_path_between(&self, point1: RoomXY, point2: RoomXY, build_step: Option<ColonyStep>, expiration_step: Option<ColonyStep>) -> Vec<Step> {
        let step = build_step.unwrap_or(ColonyStep::last());
        let expiration_step = expiration_step.unwrap_or(ColonyStep::last());

        let mut cost_matrix = self.base_cost_matrix.clone();

        for (pos, lt) in &self.roads {
            if lt.is_built_at(step) {
                cost_matrix.set_xy(*pos, TilePathing::BuiltRoad.cost());
            } else if lt.is_planned_at(step) {
                cost_matrix.set_xy(*pos, TilePathing::PlannedRoad.cost());
            }
        }

        for (pos, history) in &self.tiles {
            if history.get_impassable_step().is_some_and(|block_stop| block_stop < expiration_step) {
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
}