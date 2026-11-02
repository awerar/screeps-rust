use std::collections::{HashMap, HashSet};

use itertools::Itertools;
use screeps::{CostMatrix, Room, RoomTerrain, RoomXY, StructureType, Terrain};
use anyhow::bail;

use crate::{colony::{planner::{block_checker::BlockChecker, state::{budget::StructureBudget, entities::{PlannedBlocker, PlannedEntity}, pathing::TilePathing, tile::TileTimeline}}, steps::ColonyStep}, utils::GetXYGood};

mod budget;
mod compile;
mod entities;
mod lifetime;
mod pathing;
mod tile;

pub use entities::{PlannedContainer, PlannedCreep, PlannedImpassableStructure};
pub use lifetime::LifeTime;
pub use pathing::PathId;

// An Err from any plan_* or reserve_block call can leave the subsystems out of sync,
// so it is terminal and the planner has to be dropped
pub struct ColonyPlanner {
    pub room: Room,
    terrain: RoomTerrain,
    base_cost_matrix: CostMatrix,

    block_checker: BlockChecker,
    reserved_blocks: HashSet<RoomXY>,
    keep_clear: HashSet<RoomXY>,

    tiles: HashMap<RoomXY, TileTimeline>,

    budget: StructureBudget,
    paths: HashMap<PathId, Vec<RoomXY>>
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
            budget: StructureBudget::default(),
            tiles: HashMap::new(),
            paths: HashMap::new(),
            block_checker: BlockChecker::new(&room),
            reserved_blocks: HashSet::new(),
            keep_clear: HashSet::new(),
            room,
        }
    }

    pub fn count_left_for(&self, structure: StructureType, step: ColonyStep) -> u32 {
        self.budget.count_left_for(structure, step)
    }

    pub fn get_earliest_step_for(&mut self, structure: StructureType) -> anyhow::Result<ColonyStep> {
        self.budget.get_earliest_step_for(structure)
    }

    pub fn can_block_at(&self, xy: RoomXY) -> bool {
        self.block_checker.can_block(xy) && !self.keep_clear.contains(&xy)
    }

    pub fn is_free_at(&self, xy: RoomXY) -> bool {
        self.terrain.get_xy_good(xy) != Terrain::Wall
        && self.tiles.get(&xy).is_none_or(TileTimeline::is_free)
        && !self.reserved_blocks.contains(&xy)
    }

    // Blocks the tile now, plan_blocker has to fill it in before compiling
    pub fn reserve_block(&mut self, xy: RoomXY) -> anyhow::Result<()> {
        self.block_checker.block(xy)?;
        self.reserved_blocks.insert(xy);

        Ok(())
    }

    pub fn keep_clear(&mut self, xy: RoomXY) {
        self.keep_clear.insert(xy);
    }

    pub fn plan_road(&mut self, pos: RoomXY, lt: LifeTime) -> anyhow::Result<()> {
        self.tiles.entry(pos).or_default().plan_road(lt)
    }

    fn check_entity_plannable_at(&self, xy: RoomXY, step: ColonyStep, entity: PlannedEntity) -> anyhow::Result<()> {
        if !entity.plannable_on_wall() && self.terrain.get(xy.x.u8(), xy.y.u8()) == Terrain::Wall { bail!("Can't plan {entity:?} due to wall") }

        if let Some(structure) = entity.structure_type() && self.count_left_for(structure, step) == 0 {
            bail!("Can't plan {structure:?} due to insufficient number of buildings at {step:?}");
        }

        Ok(())
    }

    pub fn plan_container(&mut self, xy: RoomXY, lt: LifeTime, container: PlannedContainer) -> anyhow::Result<()> {
        self.check_entity_plannable_at(xy, lt.build_step(), container.into())?;

        self.tiles.entry(xy).or_default().plan_container(lt, container)?;

        self.budget.add(StructureType::Container, lt);

        Ok(())
    }

    pub fn plan_blocker(&mut self, xy: RoomXY, step: ColonyStep, blocker: PlannedBlocker) -> anyhow::Result<()> {
        self.check_entity_plannable_at(xy, step, blocker.into())?;

        if self.keep_clear.contains(&xy) { bail!("Can't plan a blocker on {xy} because it is marked as keep clear") }

        if self.terrain.get_xy_good(xy) != Terrain::Wall && !self.reserved_blocks.remove(&xy) {
            self.block_checker.block(xy)?;
        }

        self.tiles.entry(xy).or_default().plan_blocker(step, blocker)?;

        if let Some(structure) = blocker.structure_type() {
            self.budget.add(structure, LifeTime::permanent(step));
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
        let path = self.find_path_from(xy1, xy2, Some(lt.build_step()), lt.destroy_step());

        for pos in &path {
            self.plan_road(*pos, lt)?;
        }

        if let Some(id) = id {
            let old_path = self.paths.insert(id, path);
            if old_path.is_some() { bail!("Already saved a path with this id"); }
        }

        Ok(())
    }
}
