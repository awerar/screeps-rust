use std::{collections::{HashMap, HashSet}, sync::LazyLock};

use itertools::Itertools;
use log::*;
use screeps::{
    ConstructionSite, Position, ResourceType, StructureController, StructureExtension, StructureObject, StructureSpawn, StructureTower, StructureType, find, game, local::ObjectId, look::{self, LookResult}, objects::{Creep, Source}, prelude::*
};
use serde::{Deserialize, Serialize};

use crate::movement::smart_move_creep_to;

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

#[derive(Serialize, Deserialize, Debug)]
pub struct SourceAssignments {
    assignments: HashMap<String, ObjectId<Source>>,
    sources: HashMap<ObjectId<Source>, SourceData>
}

impl SourceAssignments {
    fn get_assignmemnt(&mut self, creep: &Creep) -> Option<ObjectId<Source>> {
        if let assignment@Some(_) = self.assignments.get(&creep.name()) { return assignment.cloned() }
        
        let assignment = self.sources.iter()
            .filter(|(_, source_data)| source_data.assigned.len() < source_data.capacity)
            .map(|(source,_ )| source).next().cloned();

        if let Some(assignment) = assignment {
            self.assignments.insert(creep.name(), assignment);
            self.sources.get_mut(&assignment).unwrap().assigned.insert(creep.name());
        }

        assignment
    }

    pub fn remove(&mut self, creep_name: &str) {
        self.assignments.remove(creep_name);
        for source_data in self.sources.values_mut() {
            source_data.assigned.remove(creep_name);
        }
    }

    pub fn max_creeps(&self) -> usize {
        self.sources.values().map(|source_data| source_data.capacity).sum()
    }
}

impl Default for SourceAssignments {
    fn default() -> Self {
        let room = game::spawns().values().next().unwrap().room().unwrap();
        let sources = room.find(find::SOURCES, None).into_iter()
            .map(|source| (source.id(), SourceData::default())).collect();
        Self { assignments: HashMap::new(), sources: sources }
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct SourceData {
    capacity: usize,
    assigned: HashSet<String>
}

impl Default for SourceData {
    fn default() -> Self {
        Self { capacity: 4, assigned: Default::default() }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum HarvesterState {
    Idle,
    Harvesting(ObjectId<Source>),
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

impl Default for HarvesterState {
    fn default() -> Self {
        HarvesterState::Idle
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
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

pub fn do_harvester_creep(creep: &Creep, curr_state: HarvesterState, source_distribution: &mut SourceAssignments) -> Option<HarvesterState> {
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
                Idle => info!("{} has no assignment. Idling.", creep.name()),
                _ => next_state = do_harvester_creep(creep, next_state, source_distribution)?
            }

            Some(next_state)
        },
        Harvesting(source) => {
            let source = source.resolve()?;

            smart_move_creep_to(creep, &source).ok();
            if creep.pos().is_near_to(source.pos()) {
                creep.harvest(&source).ok();
            }

            if is_full(creep) { do_harvester_creep(creep, Idle, source_distribution) }
            else { Some(curr_state) }
        },
        Distributing(target) => {
            try_repair(creep);

            let target_pos = target.pos()?;
            smart_move_creep_to(creep, target_pos).ok();

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