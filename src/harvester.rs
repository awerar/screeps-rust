use std::{collections::{HashMap, HashSet}, ops::Add, sync::LazyLock};

use itertools::Itertools;
use log::*;
use screeps::{
    ConstructionSite, Position, ResourceType, Room, StructureController, StructureExtension, StructureObject, StructureSpawn, StructureTower, StructureType, Terrain, find, game, local::ObjectId, look::{self, LookResult}, objects::{Creep, Source}, prelude::*
};
use serde::{Deserialize, Serialize};
use serde_json_any_key::*;

type HarvestAssignment = (Position, ObjectId<Source>);

extern crate serde_json_path_to_error as serde_json;

static BUILDING_PRIORITY: LazyLock<HashMap<StructureType, i32>> = LazyLock::new(|| {
    use StructureType::*;
    let priority = vec![Extension, Road];
    priority.into_iter().rev().enumerate().map(|(a, b)| (b, a as i32)).collect()
});

static FILL_PRIORITY: LazyLock<HashMap<StructureType, i32>> = LazyLock::new(|| {
    use StructureType::*;
    let priority = vec![Spawn, Extension, Tower, Storage];
    priority.into_iter().rev().enumerate().map(|(a, b)| (b, a as i32)).collect()
});

const REPAIR_THRESHOLD: f32 = 0.8;

#[derive(Serialize, Deserialize)]
pub struct SourceDistribution {
    #[serde(with = "any_key_map")] 
    pub harvest_positions: HashMap<ObjectId<Source>, SourceData>,
    pub creep_assignments: HashMap<String, HarvestAssignment>
}

#[derive(Serialize, Deserialize, Debug)]
pub struct HarvestPositionData {
    pub capacity: usize,
    pub assigned: HashSet<String>
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SourceData(#[serde(with = "any_key_map")] HashMap<Position, HarvestPositionData>);

impl SourceData {
    pub fn try_assign(&mut self, creep: &Creep) -> Option<Position> {
        let free_pos = self.0.iter()
            .map(|(pos, pos_data)| (pos_data.capacity - pos_data.assigned.len(), pos))
            .filter(|(free_space, _)| *free_space > 0)
            .sorted()
            .map(|(_, pos)| pos)
            .next()?.clone();

        self.0.get_mut(&free_pos).unwrap().assigned.insert(creep.name());
        Some(free_pos)
    }
}

impl SourceDistribution {
    pub fn new(room: Room) -> SourceDistribution {
        let harvest_positions = room.find(find::SOURCES, None).into_iter().map(|source| {
            let free_positions: Vec<_> = 
                (-1..=1).cartesian_product(-1..=1)
                .map(|offset| source.pos().add(offset))
                .filter(|pos| room.get_terrain().get_xy(pos.xy()) != Terrain::Wall).collect();

            let source_data = SourceData(
                free_positions.into_iter()
                    .map(|pos| (pos, HarvestPositionData { assigned: HashSet::new(), capacity: 1 }))
                    .collect()
            );

            (source.id(), source_data)
        }).collect();

        Self { harvest_positions, creep_assignments: HashMap::new() }
    }

    pub fn default() -> SourceDistribution {
        Self::new(game::spawns().values().next().expect("There should be at least one spawn").room().unwrap())
    }

    pub fn get_assignmemnt(&mut self, creep: &Creep) -> Option<(Position, ObjectId<Source>)> {
        if let Some(assignment) = self.creep_assignments.get(&creep.name()) { return Some(assignment.clone()) };

        let mut assignment = None;
        for (source, harvest_positions) in self.harvest_positions.iter_mut() {
            assignment = harvest_positions.try_assign(creep).map(|pos| (pos, source.clone()));
            if assignment.is_some() { break; }
        }

        if let Some(assignment) = assignment {
            info!("Assigning {} to source {}, pos={}", creep.name(), assignment.1, assignment.0);

            self.creep_assignments.insert(creep.name(), assignment);
            self.creep_assignments.get(&creep.name()).cloned()
        } else { None }
    }

    pub fn max_creeps(&self) -> usize {
        self.harvest_positions.values()
            .flat_map(|source_data| source_data.0.values())
            .map(|harvest_pos| harvest_pos.capacity)
            .sum()
    }

    pub fn cleanup_dead_creep(&mut self, dead_creep: &str) {
        self.creep_assignments.remove(dead_creep);

        for source_data in self.harvest_positions.values_mut() {
            for harvest_data in source_data.0.values_mut() {
                harvest_data.assigned.remove(dead_creep);
            }
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub enum HarvesterState {
    Idle,
    Harvesting(HarvestAssignment),
    Distributing(DistributionTarget)
}

impl HarvesterState {
    fn is_idle(&self) -> bool {
        match self {
            HarvesterState::Idle => true,
            _ => false
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub enum DistributionTarget {
    Controller(ObjectId<StructureController>), 
    Spawn(ObjectId<StructureSpawn>),
    Extension(ObjectId<StructureExtension>),
    Tower(ObjectId<StructureTower>),
    ConstructionSite(ObjectId<ConstructionSite>)
}

impl DistributionTarget {
    fn pos(&self) -> Option<Position> {
        match &self {
            DistributionTarget::Controller(object_id) => object_id.resolve().map(|x| x.pos()),
            DistributionTarget::Spawn(object_id) => object_id.resolve().map(|x| x.pos()),
            DistributionTarget::Extension(object_id) => object_id.resolve().map(|x| x.pos()),
            DistributionTarget::ConstructionSite(object_id) => object_id.resolve().map(|x| x.pos()),
            DistributionTarget::Tower(object_id) => object_id.resolve().map(|x| x.pos())
        }
    }

    fn distribute(&self, creep: &Creep) -> Option<()> {
        match &self {
            DistributionTarget::Controller(controller) => 
                creep.upgrade_controller(&controller.resolve()?).ok(),
            DistributionTarget::Spawn(spawn) => 
                creep.transfer(&spawn.resolve()?, ResourceType::Energy, None).ok(),
            DistributionTarget::Extension(extension) => 
                creep.transfer(&extension.resolve()?, ResourceType::Energy, None).ok(),
            DistributionTarget::Tower(tower) => 
                creep.transfer(&tower.resolve()?, ResourceType::Energy, None).ok(),
            DistributionTarget::ConstructionSite(site) => 
            creep.build(&site.resolve()?).ok(),
        }
    }

    fn range(&self) -> u32 {
        match self {
            DistributionTarget::ConstructionSite(_) |
            DistributionTarget::Controller(_) => 3,
            DistributionTarget::Spawn(_) |
            DistributionTarget::Extension(_) |
            DistributionTarget::Tower(_) => 1,
        }
    }
}

fn get_distribution_target(creep: &Creep) -> Option<DistributionTarget> {
    let room = creep.room()?;
    if room.controller()?.ticks_to_downgrade()? < 1000 {
        return Some(DistributionTarget::Controller(room.controller()?.id()))
    }

    let fill_target = room.find(find::MY_STRUCTURES, None).into_iter()
        .filter(|structure| {
            let Some(has_store) = structure.as_has_store() else { return false };
            has_store.store().get_free_capacity(Some(ResourceType::Energy)) > 0
        })
        .max_set_by_key(|structure| FILL_PRIORITY.get(&structure.structure_type()).unwrap_or(&-1)).into_iter()
        .min_by_key(|site| site.pos().get_range_to(creep.pos()));
        
    if let Some(fill_target) = fill_target {
        let target = match fill_target {
            StructureObject::StructureSpawn(spawn) => DistributionTarget::Spawn(spawn.id()),
            StructureObject::StructureExtension(extension) => DistributionTarget::Extension(extension.id()),
            StructureObject::StructureTower(tower) => DistributionTarget::Tower(tower.id()),
            _ => {
                warn!("Unknown structure to fill: {}", fill_target.structure_type());
                return None
            }
        };

        return Some(target)
    }

    let site = room.find(find::CONSTRUCTION_SITES, None).into_iter()
        .max_set_by_key(|site| BUILDING_PRIORITY.get(&site.structure_type()).unwrap_or(&-1)).into_iter()
        .min_by_key(|site| site.pos().get_range_to(creep.pos()));
    if let Some(site) = site { 
        if let Some(site_id) = site.try_id() { 
            return Some(DistributionTarget::ConstructionSite(site_id)); 
        }
    }

    Some(DistributionTarget::Controller(room.controller()?.id()))
}

fn is_full(creep: &Creep) -> bool {
    creep.store().get_free_capacity(None) == 0
}

fn is_empty(creep: &Creep) -> bool {
    creep.store().get_used_capacity(None) == 0
}

fn try_repair(creep: &Creep) -> Option<()> {
    let room = creep.room()?;

    let min_pos: (u8, u8) = (creep.pos() - (3, 3)).into();
    let max_pos: (u8, u8) = (creep.pos() + (3, 3)).into();
    let repair_structures: Vec<_> = room.look_for_at_area(look::STRUCTURES, min_pos.1, min_pos.0, max_pos.1, max_pos.0).into_iter()
        .map(|look| {
            let LookResult::Structure(structure) = look.look_result else { unreachable!() };
            structure
        })
        .filter(|structure| if let StructureType::Road = structure.structure_type() { true } else { false })
        .filter(|structure| structure.hits() <= ((structure.hits_max() as f32) * REPAIR_THRESHOLD) as u32)
        .collect();

    for structure in repair_structures {
        let structure = StructureObject::from(structure);
        let Some(repairable) = structure.as_repairable() else { continue; };
        if creep.repair(repairable).is_err() {
            break;
        }
    }

    Some(())
}

pub fn do_harvester_creep(creep: &Creep, curr_state: HarvesterState, source_distribution: &mut SourceDistribution) -> Option<HarvesterState> {
    use HarvesterState::*;
    
    match &curr_state {
        Idle => {
            let mut next_state = Idle;

            if !is_empty(creep) {
                if let Some(target) = get_distribution_target(creep) {
                    next_state = Distributing(target)
                }
            }

            if next_state.is_idle() && !is_full(creep) {
                if let Some(assignment) = source_distribution.get_assignmemnt(creep) {
                    next_state = Harvesting(assignment)
                }
            }

            match next_state {
                Idle => warn!("{} has no assignment. Idling.", creep.name()),
                _ => next_state = do_harvester_creep(creep, next_state, source_distribution)?
            }

            Some(next_state)
        },
        Harvesting((pos, source)) => {
            creep.move_to(*pos).ok();
            if creep.pos().is_near_to(*pos) {
                let source = source.resolve()?;
                creep.harvest(&source).ok();
            }

            if is_full(creep) { do_harvester_creep(creep, Idle, source_distribution) }
            else { Some(curr_state) }
        },
        Distributing(target) => {
            try_repair(creep);

            let target_pos = target.pos()?;
            creep.move_to(target_pos).ok();

            if creep.pos().get_range_to(target_pos) <= target.range() {
                if target.distribute(creep).is_none() {
                    return do_harvester_creep(creep, Idle, source_distribution)
                }
            }

            if let DistributionTarget::ConstructionSite(site) = target {
                if site.resolve().is_none() {
                    return do_harvester_creep(creep, Idle, source_distribution)
                }
            }

            if is_empty(creep) { do_harvester_creep(creep, Idle, source_distribution) }
            else { Some(curr_state) }
        },
    }
}