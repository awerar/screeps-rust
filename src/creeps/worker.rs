use std::{collections::HashMap, sync::LazyLock};

use itertools::Itertools;
use js_sys::Math::random;
use log::warn;
use screeps::{
    ConstructionSite, Position, Resource, ResourceType, StructureController, StructureExtension, StructureObject, StructureSpawn, StructureStorage, StructureTerminal, StructureTower, StructureType, action_error_codes::HarvestErrorCode, find, local::ObjectId, objects::{Creep, Source}, prelude::*
};
use serde::{Deserialize, Serialize};

use crate::{colony::ColonyData, memory::Memory, statemachine::StateMachine};

extern crate serde_json_path_to_error as serde_json;

static BUILDING_PRIORITY: LazyLock<HashMap<StructureType, i32>> = LazyLock::new(|| {
    use StructureType::*;
    let priority = vec![Extension, Container, Tower, Road, Storage, Terminal];
    priority.into_iter().rev().enumerate().map(|(a, b)| (b, a as i32)).collect()
});

static FILL_PRIORITY: LazyLock<HashMap<StructureType, i32>> = LazyLock::new(|| {
    use StructureType::*;
    let priority = vec![Spawn, Extension, Tower, Terminal, Storage];
    priority.into_iter().rev().enumerate().map(|(a, b)| (b, a as i32)).collect()
});

const REPAIR_THRESHOLD: f32 = 0.8;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
pub enum WorkerCreep {
    #[default]
    Idle,
    Harvesting(ObjectId<Source>),
    PickingUp(ObjectId<Resource>),
    Distributing(DistributionTarget)
}

impl WorkerCreep {
    fn is_idle(&self) -> bool {
        matches!(self, WorkerCreep::Idle)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum DistributionTarget {
    Controller(ObjectId<StructureController>), 
    Spawn(ObjectId<StructureSpawn>),
    Extension(ObjectId<StructureExtension>),
    Tower(ObjectId<StructureTower>),
    Storage(ObjectId<StructureStorage>),
    Terminal(ObjectId<StructureTerminal>),
    ConstructionSite(ObjectId<ConstructionSite>),
}

impl DistributionTarget {
    fn pos(&self) -> Option<Position> {
        match &self {
            DistributionTarget::Controller(object_id) => object_id.resolve().map(|x| x.pos()),
            DistributionTarget::Spawn(object_id) => object_id.resolve().map(|x| x.pos()),
            DistributionTarget::Extension(object_id) => object_id.resolve().map(|x| x.pos()),
            DistributionTarget::ConstructionSite(object_id) => object_id.resolve().map(|x| x.pos()),
            DistributionTarget::Tower(object_id) => object_id.resolve().map(|x| x.pos()),
            DistributionTarget::Storage(object_id) => object_id.resolve().map(|x| x.pos()),
            DistributionTarget::Terminal(object_id) => object_id.resolve().map(|x| x.pos()),
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
            DistributionTarget::Storage(storage) => 
                creep.transfer(&storage.resolve()?, ResourceType::Energy, None).ok(),
            DistributionTarget::Terminal(terminal) => 
                creep.transfer(&terminal.resolve()?, ResourceType::Energy, None).ok(),
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
            DistributionTarget::Storage(_) |
            DistributionTarget::Terminal(_) |
            DistributionTarget::Tower(_) => 1,
        }
    }

    fn still_valid(&self) -> bool {
        match self {
            DistributionTarget::Controller(_) => true,
            DistributionTarget::Spawn(spawn) => 
                spawn.resolve().is_some_and(|spawn| spawn.store().get_free_capacity(Some(ResourceType::Energy)) > 0),
            DistributionTarget::Extension(extension) => 
                extension.resolve().is_some_and(|extension| extension.store().get_free_capacity(Some(ResourceType::Energy)) > 0),
            DistributionTarget::Tower(tower) => 
                tower.resolve().is_some_and(|tower| tower.store().get_free_capacity(Some(ResourceType::Energy)) > 0),
            DistributionTarget::Storage(storage) => 
                storage.resolve().is_some_and(|storage| storage.store().get_free_capacity(Some(ResourceType::Energy)) > 0),
            DistributionTarget::Terminal(terminal) => 
                terminal.resolve().is_some_and(|terminal| terminal.store().get_free_capacity(Some(ResourceType::Energy)) > 0),
            DistributionTarget::ConstructionSite(site) => site.resolve().is_some(),
        }
    }
}

fn get_distribution_target(creep: &Creep) -> Option<DistributionTarget> {
    let room = creep.room()?;
    if room.controller()?.ticks_to_downgrade()? < 5000 {
        return Some(DistributionTarget::Controller(room.controller()?.id()))
    }

    let fill_target = room.find(find::MY_STRUCTURES, None).into_iter()
        .filter(|structure| {
            let Some(has_store) = structure.as_has_store() else { return false };
            has_store.store().get_free_capacity(Some(ResourceType::Energy)) > 0 && 
            has_store.store().get_used_capacity(Some(ResourceType::Energy)) < 50000
        })
        .filter(|structure| FILL_PRIORITY.contains_key(&structure.structure_type()))
        .max_set_by_key(|structure| FILL_PRIORITY.get(&structure.structure_type()).unwrap_or(&-1)).into_iter()
        .min_by_key(|site| site.pos().get_range_to(creep.pos()));
        
    if let Some(fill_target) = fill_target {
        let target = match fill_target {
            StructureObject::StructureSpawn(spawn) => DistributionTarget::Spawn(spawn.id()),
            StructureObject::StructureExtension(extension) => DistributionTarget::Extension(extension.id()),
            StructureObject::StructureTower(tower) => DistributionTarget::Tower(tower.id()),
            StructureObject::StructureStorage(storage) => DistributionTarget::Storage(storage.id()),
            StructureObject::StructureTerminal(terminal) => DistributionTarget::Terminal(terminal.id()),
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
    let structures = creep.pos().find_in_range(find::STRUCTURES, 3);
    let repair_structures: Vec<_> = structures.iter()
        .filter(|structure| matches!(structure.structure_type(), StructureType::Road))
        .filter_map(|structure| structure.as_repairable())
        .filter(|repairable| repairable.hits() <= ((repairable.hits_max() as f32) * REPAIR_THRESHOLD) as u32)
        .collect();

    for repairable in repair_structures {
        if creep.repair(repairable).is_err() {
            break;
        }
    }

    Some(())
}

impl StateMachine<Creep> for WorkerCreep {
    fn update(&self, creep: &Creep, mem: &mut Memory) -> Result<Self, ()> {
        use WorkerCreep::*;
    
        match &self {
            Idle => {
                let mut next_state = Idle;

                if !is_empty(creep) {
                    if let Some(target) = get_distribution_target(creep) {
                        next_state = Distributing(target);
                    }
                }

                if next_state.is_idle() && !is_full(creep) {
                    let sources = mem.creep_home(creep).ok_or(())?.room().ok_or(())?.find(find::SOURCES_ACTIVE, None);
                    /*let sources: Vec<_> = sources.into_iter().filter(|source| {
                        (-1..=1).cartesian_product(-1..=1).map(|offset| source.pos() + offset)
                            .map(|pos| pos.look_for(look::CREEPS)
                                .map_or(true, |creeps| creeps.len() == 0))
                            .any(|x| x)
                    }).collect();*/

                    if !sources.is_empty() {
                        let source = &sources[(random() * (sources.len() as f64)).floor() as usize];
                        next_state = Harvesting(source.id());
                    }

                    if let Some(resource) = creep.room().ok_or(())?.find(find::DROPPED_RESOURCES, None).into_iter().min_by_key(screeps::Resource::amount) {
                        next_state = PickingUp(resource.id());
                    }
                }

                Ok(next_state)
            },
            Harvesting(source) => {
                let source = source.resolve().ok_or(())?;

                mem.movement.smart_move_creep_to(creep, &source).ok();
                if creep.pos().is_near_to(source.pos()) {
                    use HarvestErrorCode::*;
                    if let Err(Tired) = creep.harvest(&source) {
                        return Ok(Idle)
                    }
                }

                if is_full(creep) { Ok(Idle) }
                else { Ok(self.clone()) }
            },
            PickingUp(resource) => {
                let Some(resource) = resource.resolve() else { return Ok(Idle) };

                if creep.pos().is_near_to(resource.pos()) {
                    creep.pickup(&resource).ok();
                    Ok(Idle)
                } else {
                    mem.movement.smart_move_creep_to(creep, &resource).ok();
                    Ok(self.clone())
                }
            }
            Distributing(target) => {
                if !(matches!(target, DistributionTarget::Controller(_)) 
                    && mem.creep_home(creep)
                        .and_then(ColonyData::controller)
                        .and_then(|controller| controller.ticks_to_downgrade())
                        .is_some_and(|ticks| ticks < 5000)) {
                    try_repair(creep);
                }

                if !target.still_valid() { return Ok(Idle) }

                let target_pos = target.pos().ok_or(())?;
                mem.movement.smart_move_creep_to(creep, target_pos).ok();

                if creep.pos().get_range_to(target_pos) <= target.range()
                    && target.distribute(creep).is_none() {
                        return Ok(Idle)
                    }

                if let DistributionTarget::ConstructionSite(site) = target {
                    if site.resolve().is_none() {
                        return Ok(Idle)
                    }
                }

                if is_empty(creep) { Ok(Idle) }
                else { Ok(self.clone()) }
            },
        }
    }
}