use std::collections::{BTreeMap, HashMap, HashSet};

use itertools::Itertools;
use option_entry::OptionEntry;
use screeps::{CostMatrix, CostMatrixSet, FindPathOptions, HasPosition, Path, Position, Room, RoomCoordinate, RoomTerrain, RoomXY, Source, StructureType, Terrain, find, pathfinder::SingleRoomCostResult};
use serde::{Deserialize, Serialize};
use anyhow::{anyhow, bail};
use strum::IntoEnumIterator;

use crate::{colony::{plan::{CenterPlan, ColonyPlan, ColonyPlanStep, MineralPlan, SourcePlan, SourcesPlan, refs::{PlannedStructureRef, PlannedStructureRefs}}, planner::block_checker::BlockChecker, steps::ColonyStep}, domain_traits::HasId, utils::GetXYGood};

// TODO: Split this into many smaller files

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Clone, Copy, Debug)]
pub enum PlannedImpassableStructure {
    MainSpawn,
    SourceSpawn(screeps::ObjectId<Source>),
    SourceLink(screeps::ObjectId<Source>),
    SourceExtension(screeps::ObjectId<Source>),
    Extension,
    Storage,
    Terminal,
    Tower,
    CentralLink,
    Extractor,
    Observer
}

impl PlannedImpassableStructure {
    fn buildable_on_wall(&self) -> bool {
        use PlannedImpassableStructure::*;

        matches!(self, Extractor)
    }

    fn structure_type(&self) -> StructureType {
        use StructureType::*;

        match self {
            PlannedImpassableStructure::MainSpawn
            | PlannedImpassableStructure::SourceSpawn(_) => Spawn,
            PlannedImpassableStructure::Extension
            | PlannedImpassableStructure::SourceExtension(_) => Extension,
            PlannedImpassableStructure::CentralLink
            | PlannedImpassableStructure::SourceLink(_) => Link,
            PlannedImpassableStructure::Storage => Storage,
            PlannedImpassableStructure::Tower => Tower,
            PlannedImpassableStructure::Terminal => Terminal,
            PlannedImpassableStructure::Extractor => Extractor,
            PlannedImpassableStructure::Observer => Observer
        }
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Clone, Copy, Debug)]
pub enum PlannedContainer {
    Container,
    Source(screeps::ObjectId<Source>),
    Mineral,
    Upgrader
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Clone, Copy, Debug)]
pub enum PlannedStructure {
    Container(PlannedContainer),
    Impassable(PlannedImpassableStructure)
}

impl From<PlannedImpassableStructure> for PlannedStructure { fn from(value: PlannedImpassableStructure) -> Self { Self::Impassable(value) } }
impl From<PlannedContainer> for PlannedStructure { fn from(value: PlannedContainer) -> Self { Self::Container(value) } }

impl PlannedStructure {
    #[expect(unused)]
    fn structure_type(&self) -> StructureType {
        match self {
            PlannedStructure::Container(structure) => StructureType::Container,
            PlannedStructure::Impassable(structure) => structure.structure_type(),
        }
    }
}

impl From<PlannedStructure> for PlannedEntity {
    fn from(value: PlannedStructure) -> Self {
        match value {
            PlannedStructure::Container(structure) => PlannedEntity::Container(structure),
            PlannedStructure::Impassable(structure) => PlannedEntity::Blocker(structure.into()),
        }
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Clone, Copy, Debug)]
pub enum PlannedCreep {
    Upgrader,
    SourceHarvester(screeps::ObjectId<Source>),
    MineralHarvester
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Clone, Copy, Debug)]
pub enum PlannedBlocker {
    Creep(PlannedCreep),
    Structure(PlannedImpassableStructure)
}

impl PlannedBlocker {
    fn is_creep(&self) -> bool {
        matches!(self, Self::Creep(_))
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

impl From<PlannedImpassableStructure> for PlannedBlocker { fn from(value: PlannedImpassableStructure) -> Self { Self::Structure(value) } }
impl From<PlannedCreep> for PlannedBlocker { fn from(value: PlannedCreep) -> Self { Self::Creep(value) } }

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Clone, Copy, Debug)]
pub enum PlannedEntity {
    Blocker(PlannedBlocker),
    Container(PlannedContainer)
}

impl PlannedEntity {
    fn structure_type(&self) -> Option<StructureType> {
        match self {
            PlannedEntity::Blocker(blocker) => blocker.structure_type(),
            PlannedEntity::Container(_) => Some(StructureType::Container),
        }
    }

    fn plannable_on_wall(&self) -> bool {
        match self {
            PlannedEntity::Blocker(blocker) => blocker.plannable_on_wall(),
            PlannedEntity::Container(_) => false,
        }
    }
}

impl From<PlannedBlocker> for PlannedEntity { fn from(value: PlannedBlocker) -> Self { Self::Blocker(value) }}
impl From<PlannedContainer> for PlannedEntity { fn from(value: PlannedContainer) -> Self { Self::Container(value) }}
impl From<PlannedImpassableStructure> for PlannedEntity { fn from(value: PlannedImpassableStructure) -> Self { Self::Blocker(PlannedBlocker::Structure(value)) }}
impl From<PlannedCreep> for PlannedEntity { fn from(value: PlannedCreep) -> Self { Self::Blocker(PlannedBlocker::Creep(value)) }}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct TileTimeline {
    blocker: Option<(ColonyStep, PlannedBlocker)>,
    container: Option<(LifeTime, PlannedContainer)>,
    road: Option<LifeTime>
}

impl TileTimeline {
    fn get_block_step(&self) -> Option<ColonyStep> {
        self.blocker.map(|(step, _) | step)
    }

    fn is_free(&self) -> bool {
        self.blocker.is_none() && self.container.is_none()
    }

    fn is_container_blocker_compatible(container_lt: LifeTime, block_step: ColonyStep, blocker: PlannedBlocker) -> bool {
        blocker.is_creep() || container_lt.is_destroyed_at(block_step)
    }

    fn plan_container(&mut self, lt: LifeTime, container: PlannedContainer) -> anyhow::Result<()> {
        if self.blocker.is_some_and(|(step, blocker)| !Self::is_container_blocker_compatible(lt, step, blocker)) { bail!("Container collides with blocker") }
        
        match self.container.entry() {
            option_entry::Entry::Occupied(_) => bail!("Already planned a container"),
            option_entry::Entry::Vacant(entry) => entry.insert((lt, container)),
        };

        Ok(())
    }

    fn plan_blocker(&mut self, step: ColonyStep, blocker: PlannedBlocker) -> anyhow::Result<()> {
        if self.container.is_some_and(|(lt, _)| !Self::is_container_blocker_compatible(lt, step, blocker)) { bail!("Blocker collides with container") }

        match self.blocker.entry() {
            option_entry::Entry::Occupied(_) => bail!("Already planned a blocker"),
            option_entry::Entry::Vacant(entry) => entry.insert((step, blocker)),
        };

        if let Some(lt) = self.road.as_mut() {
            *lt = lt.clip_stop(step).ok_or_else(|| anyhow!("Unable to move road earlier for blocker"))?;
        }

        Ok(())
    }

    fn plan_road(&mut self, mut lt: LifeTime) -> anyhow::Result<()> {
        if let Some(old_lt) = self.road.take() {
            lt = lt.union(old_lt);
        }

        if let Some((block_step, _)) = self.blocker {
            lt = lt.clip_stop(block_step).ok_or_else(|| anyhow!("Unable to move road earlier for blocker"))?;
        }

        self.road = Some(lt);

        Ok(())
    }

    fn structures(&self) -> impl Iterator<Item = (LifeTime, StructureType)> {
        self.blocker.iter().filter_map(|(step, blocker)| {
                let structure = blocker.structure_type()?;
                Some((LifeTime::permanent(*step), structure))
            }).chain(self.container.iter().map(|(lt, _)| (*lt, StructureType::Container)))
            .chain(self.road.iter().map(|lt| (*lt, StructureType::Road)))
    }

    fn entities(&self) -> impl Iterator<Item = PlannedEntity> {
        self.blocker.iter().map(|(_, blocker)| (*blocker).into())
            .chain(self.container.iter().map(|(_, container)| (*container).into()))
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

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub struct LifeTime {
    pub build_step: ColonyStep,
    pub destroy_step: Option<ColonyStep>
}

impl LifeTime {
    pub fn permanent(build: ColonyStep) -> Self {
        LifeTime { build_step: build, destroy_step: None }
    }

    pub fn temporary(build: ColonyStep, destroy: ColonyStep) -> Option<Self> {
        if destroy <= build { return None }

        Some(LifeTime { build_step: build, destroy_step: Some(destroy) })
    }

    pub fn is_built_at(self, step: ColonyStep) -> bool {
        step >= self.build_step && self.destroy_step.is_none_or(|destroy_step| destroy_step > step)
    }

    pub fn is_planned_at(self, step: ColonyStep) -> bool {
        step < self.build_step
    }

    pub fn is_destroyed_at(self, step: ColonyStep) -> bool {
        self.destroy_step.is_some_and(|destroy_step| step >= destroy_step)
    }

    pub fn union(self, other: Self) -> Self {
        Self { 
            build_step: self.build_step.min(other.build_step), 
            destroy_step: self.destroy_step.zip(other.destroy_step).map(|(s1, s2)| s1.max(s2))
        }
    }

    pub fn clip_start(self, min_build_step: ColonyStep) -> Option<Self> {
        let build_step = self.build_step.max(min_build_step);
        self.destroy_step.map_or(
            Some(LifeTime::permanent(build_step)), 
            |destroy_step| LifeTime::temporary(build_step, destroy_step)
        )
    }

    pub fn clip_stop(self, max_destroy_step: ColonyStep) -> Option<Self> {
        LifeTime::temporary(
            self.build_step, 
            self.destroy_step.map_or(max_destroy_step, |destroy_step| destroy_step.min(max_destroy_step))
        )
    }
}

pub struct IntermediateColonyPlan {
    pub room: Room,

    pub new_structures: HashMap<ColonyStep, HashSet<(RoomXY, StructureType)>>,
    pub expired_structures: HashMap<ColonyStep, HashSet<(RoomXY, StructureType)>>,

    pub entities2pos: HashMap<PlannedEntity, Vec<RoomXY>>,
    pub paths: HashMap<PathId, Vec<RoomXY>>
}

impl IntermediateColonyPlan {
    fn compile(self) -> anyhow::Result<ColonyPlan> {
        let mut plan_steps = HashMap::new();

        let mut accumulated_structures = HashSet::new();
        for step in ColonyStep::iter() {
            let new_structures = self.new_structures.get(&step).cloned().unwrap_or_default();
            let expired_structures = self.expired_structures.get(&step).cloned().unwrap_or_default();

            accumulated_structures.extend(new_structures.iter().copied());
            accumulated_structures = accumulated_structures.difference(&expired_structures).copied().collect();

            plan_steps.insert(step, ColonyPlanStep { 
                new_structures, 
                expired_structures, 
                accumulated_structures: accumulated_structures.clone()
            });
        }

        let center = self.compile_center()?;

        Ok(ColonyPlan {
            steps: plan_steps,
            sources: self.compile_sources()?,
            mineral: self.compile_mineral()?,
            center,
            upgrader_container: self.get_structure_ref(PlannedContainer::Upgrader.into())?,
            controller: PlannedStructureRef::new(self.room.controller().ok_or(anyhow!("No controller"))?.pos())
        })
    }


    fn compile_center(&self) -> anyhow::Result<CenterPlan> {
        use PlannedImpassableStructure::*;
        use PlannedContainer::*;

        let storage_ref = self.get_structure_ref(Storage.into())?;

        Ok(CenterPlan {
            pos: storage_ref.pos,
            spawn: self.get_structure_ref(MainSpawn.into())?,
            storage: storage_ref.into(),
            container_storage: self.get_structure_ref(Container.into())?.into(),
            link: self.get_structure_ref(CentralLink.into())?.into(),
            terminal: self.get_structure_ref(Terminal.into())?.into(),
            observer: self.get_structure_ref(Observer.into())?.into(),
            towers: self.get_structure_refs(Tower.into()),
            extensions: self.get_structure_refs(Extension.into())
        })
    }

    fn compile_sources(&self) -> anyhow::Result<SourcesPlan> {
        use PlannedImpassableStructure::*;
        use PlannedContainer::*;

        self.room.find(find::SOURCES, None).into_iter()
            .map(|source| screeps::HasId::id(&source))
            .map(|source| {
                let container = self.get_structure_ref(Source(source).into())?;
                let spawn = self.get_structure_ref(SourceSpawn(source).into())?;

                let plan = SourcePlan {
                    spawn_direction: spawn.pos.get_direction_to(container.pos).unwrap(),
                    distance: self.get_path_length(&PathId::Source(source))?,
                    spawn: spawn.into(),
                    container: container.into(),
                    link: self.get_structure_ref(SourceLink(source).into())?.into(),
                    extensions: self.get_structure_refs(SourceExtension(source).into()),
                };

                Ok((source, plan))
            }).collect::<anyhow::Result<_>>()
    }

    fn compile_mineral(&self) -> anyhow::Result<MineralPlan> {
        use PlannedContainer::*;
        use PlannedImpassableStructure::*;

        let container = self.get_structure_ref(Mineral.into())?;

        Ok(MineralPlan {
            distance: self.get_path_length(&PathId::Mineral)?,
            container: container.into(),
            extractor: self.get_structure_ref(Extractor.into())?.into(),
        })
    }

    fn get_pos(&self, entity: PlannedEntity) -> anyhow::Result<Position> {
        self.entities2pos.get(&entity)
            .ok_or(anyhow!("No {entity:?} was found"))
            .and_then(|positions| {
                if positions.is_empty() { Err(anyhow!("No {entity:?} was found")) }
                else if positions.len() > 1 { Err(anyhow!("Unable to determine unique {entity:?}")) }
                else { Ok(*positions.iter().next().unwrap()) }
            }).map(|pos| Position::new(pos.x, pos.y, self.room.name()))
    }

    fn get_structure_ref<T: HasId>(&self, structure: PlannedStructure) -> anyhow::Result<PlannedStructureRef<T>> {
        self.get_pos(structure.into())
            .map(|pos| PlannedStructureRef::new(pos))
    }

    fn get_structure_refs<T: HasId>(&self, structure: PlannedStructure) -> PlannedStructureRefs<T> {
        let Some(positions) = self.entities2pos.get(&structure.into()) else { return PlannedStructureRefs(Vec::new()) };

        PlannedStructureRefs(positions.iter()
            .copied()
            .map(|pos| PlannedStructureRef::new(Position::new(pos.x, pos.y, self.room.name())))
            .collect())
    }

    fn get_path_length(&self, id: &PathId) -> anyhow::Result<u32> {
        Ok(self.paths.get(id).ok_or(anyhow!("No path found"))?.len() as u32)
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PathId {
    Mineral,
    Source(screeps::ObjectId<Source>)
}

#[derive(Debug)]
pub struct WallConnection {
    pub creep_pos: RoomXY,
    pub access_pos: RoomXY
}

pub struct ColonyPlanner {
    pub room: Room,
    pub terrain: RoomTerrain,
    pub base_cost_matrix: CostMatrix,

    pub block_checker: BlockChecker,

    pub tiles: HashMap<RoomXY, TileTimeline>,

    pub structure_steps: HashMap<StructureType, BTreeMap<ColonyStep, i32>>,
    pub paths: HashMap<PathId, Vec<RoomXY>>
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
            structure_steps: HashMap::new(),
            tiles: HashMap::new(),
            paths: HashMap::new(),
            block_checker: BlockChecker::new(&room),
            room,
        }
    }

    fn compile_intermediate(self) -> IntermediateColonyPlan {
        let mut new_structures: HashMap<_, HashSet<_>> = HashMap::new();
        let mut expired_structures: HashMap<_, HashSet<_>> = HashMap::new();

        for (pos, timeline) in &self.tiles {
            for (lt, structure) in timeline.structures() {
                new_structures.entry(lt.build_step).or_default().insert((*pos, structure));
                if let Some(destroy_step) = lt.destroy_step {
                    expired_structures.entry(destroy_step).or_default().insert((*pos, structure));
                }
            }
        }

        let entities2pos = self.tiles.iter()
            .flat_map(|(xy, timeline)| {
                timeline.entities().map(|entity| (entity, *xy))
            }).into_group_map();

        let controller = self.room.controller().unwrap().pos().xy();
        new_structures.entry(ColonyStep::BuildSpawn).or_default().insert((controller, StructureType::Controller));

        IntermediateColonyPlan { 
            room: self.room, 
            new_structures, 
            expired_structures, 
            entities2pos, 
            paths: self.paths
        }
    }

    pub fn compile(self) -> anyhow::Result<ColonyPlan> {
        self.compile_intermediate().compile()
    }

    pub fn count_left_for(&self, structure: StructureType, step: ColonyStep) -> u32 {
        self.calculate_all_avaliable_at(structure).into_iter()
            .skip_while(|(s, _)| *s < step)
            .map(|(_, avaliable)| avaliable)
            .min().unwrap()
    }

    pub fn get_earliest_step_for(&mut self, structure: StructureType) -> anyhow::Result<ColonyStep> {
        Ok(self.calculate_all_avaliable_at(structure).into_iter()
            .rev()
            .take_while(|(_, avaliable)| *avaliable > 0)
            .last().ok_or(anyhow!("Unable to build any more {structure:?}"))?.0)
    }

    pub fn can_block_at(&self, xy: RoomXY) -> bool {
        self.block_checker.can_block(xy)
    }

    pub fn is_free_at(&self, xy: RoomXY) -> bool {
        self.terrain.get_xy_good(xy) != Terrain::Wall
        && self.tiles.get(&xy).is_none_or(TileTimeline::is_free)
    }

    fn add_structure_lifetime(&mut self, structure: StructureType, lt: LifeTime) {
        let steps = self.structure_steps.entry(structure).or_default();
        *steps.entry(lt.build_step).or_default() += 1;
        if let Some(destroy_step) = lt.destroy_step {
            *steps.entry(destroy_step).or_default() -= 1;
        }
    }

    pub fn plan_road(&mut self, pos: RoomXY, lt: LifeTime) -> anyhow::Result<()> {
        self.tiles.entry(pos).or_default().plan_road(lt)
    }

    fn calculate_all_avaliable_at(&self, structure: StructureType) -> BTreeMap<ColonyStep, u32> {
        let default_build_steps = BTreeMap::new();
        let build_steps = self.structure_steps.get(&structure).unwrap_or(&default_build_steps);

        ColonyStep::iter()
            .map(|step| (step, build_steps.get(&step).copied().unwrap_or(0)))
            .fold((0_u32, BTreeMap::new()), |(mut count_acc, mut map_acc), (step, count)| {
                count_acc = count_acc.checked_add_signed(count).unwrap();

                let max = structure.controller_structures(u32::from(step.controller_level()));
                let avaliable = max - count_acc;
                map_acc.insert(step,  avaliable);

                (count_acc, map_acc)
            }).1
    }

    fn check_entity_plannable_at(&self, xy: RoomXY, step: ColonyStep, entity: PlannedEntity) -> anyhow::Result<()> {
        if !entity.plannable_on_wall() && self.terrain.get(xy.x.u8(), xy.y.u8()) == Terrain::Wall { bail!("Can't plan {entity:?} due to wall") }

        if let Some(structure) = entity.structure_type() && self.count_left_for(structure, step) == 0 {
            bail!("Can't plan {structure:?} due to insufficient number of buildings at {step:?}");
        }

        Ok(())
    }

    pub fn plan_container(&mut self, xy: RoomXY, lt: LifeTime, container: PlannedContainer) -> anyhow::Result<()> {
        self.check_entity_plannable_at(xy, lt.build_step, container.into())?;

        self.tiles.entry(xy).or_default().plan_container(lt, container)?;

        self.add_structure_lifetime(StructureType::Container, lt);

        Ok(())
    }

    pub fn plan_blocker(&mut self, xy: RoomXY, step: ColonyStep, blocker: PlannedBlocker) -> anyhow::Result<()> {
        self.check_entity_plannable_at(xy, step, blocker.into())?;

        if self.terrain.get_xy_good(xy) != Terrain::Wall && !self.block_checker.has_blocked_at(xy) { 
            self.block_checker.block(xy)?; 
        }

        self.tiles.entry(xy).or_default().plan_blocker(step, blocker)?;

        if let Some(structure) = blocker.structure_type() {
            self.add_structure_lifetime(structure, LifeTime::permanent(step));
        }

        Ok(())
    }

    pub fn plan_blocker_earliest(&mut self, xy: RoomXY, blocker: PlannedImpassableStructure) -> anyhow::Result<ColonyStep> {
        let earliest = self.get_earliest_step_for(blocker.structure_type())?;
        self.plan_blocker(xy, earliest, blocker.into()).map(|()| earliest)
    }

    pub fn find_all(&self, entity: PlannedEntity) -> Vec<RoomXY> {
        self.tiles.iter()
            .filter_map(|(xy, history)| {
                history.entities().contains(&entity).then_some(*xy)
            }).collect()
    }

    pub fn plan_road_from(&mut self, xy1: RoomXY, xy2: RoomXY, lt: LifeTime, id: Option<PathId>) -> anyhow::Result<()> {
        let path = self.find_path_from(xy1, xy2, Some(lt.build_step), lt.destroy_step);

        for pos in &path {
            self.plan_road(*pos, lt)?;
        }

        if let Some(id) = id {
            let old_path = self.paths.insert(id, path);
            if old_path.is_some() { bail!("Already saved a path with this id"); }
        }

        Ok(())
    }

    // Includes start, excludes end
    pub fn find_path_from(&self, xy1: RoomXY, xy2: RoomXY, build_step: Option<ColonyStep>, expiration_step: Option<ColonyStep>) -> Vec<RoomXY> {
        let build_step = build_step.unwrap_or(ColonyStep::last());
        let expiration_step = expiration_step.unwrap_or(ColonyStep::last());

        let mut cost_matrix = self.base_cost_matrix.clone();

        for (pos, timeline) in &self.tiles {
            if let Some(lt) = &timeline.road {
                if lt.is_built_at(build_step) {
                    cost_matrix.set_xy(*pos, TilePathing::BuiltRoad.cost());
                } else if lt.is_planned_at(build_step) {
                    cost_matrix.set_xy(*pos, TilePathing::PlannedRoad.cost());
                }
            }
        }

        for (pos, history) in &self.tiles {
            if history.get_block_step().is_some_and(|block_stop| block_stop < expiration_step) {
                cost_matrix.set_xy(*pos, TilePathing::Impassable.cost());
            }
        }

        let options = FindPathOptions::<fn(_, CostMatrix) -> SingleRoomCostResult, SingleRoomCostResult>::default()
            .cost_callback(|_, _| SingleRoomCostResult::CostMatrix(cost_matrix.clone()))
            .range(1);

        let pos1 = Position::new(xy1.x, xy1.y, self.room.name());
        let pos2 = Position::new(xy2.x, xy2.y, self.room.name());

        let path = pos1.find_path_to(&pos2, Some(options));

        let Path::Vectorized(path) = path else { unreachable!() };

        let mut return_path = vec![xy1];
        return_path.extend(path.into_iter().map(|step| 
            RoomXY::new(
                RoomCoordinate::new(step.x as u8).unwrap(),
                RoomCoordinate::new(step.y as u8).unwrap()
            )
        ));

        if return_path.last().is_some_and(|last| *last == xy2) {
            return_path.pop();
        }

        return_path
    }

    pub fn find_wall_connection(&self, center_pos: RoomXY, wall_pos: RoomXY, build_step: Option<ColonyStep>, expiration_step: Option<ColonyStep>) -> anyhow::Result<WallConnection> {
        let path = self.find_path_from(center_pos, wall_pos, build_step, expiration_step);
        let ending = path.last_chunk::<2>().ok_or(anyhow!("Path to wall had too few positions"))?;

        Ok(WallConnection { creep_pos: ending[1], access_pos: ending[0] })
    }
}