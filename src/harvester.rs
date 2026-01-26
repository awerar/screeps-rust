use std::{collections::{HashMap, HashSet}, ops::Add};

use itertools::Itertools;
use log::*;
use screeps::{
    Position, ResourceType, Room, StructureController, StructureSpawn, Terrain, find, game, local::ObjectId, objects::{Creep, Source}, prelude::*
};
use serde::{Deserialize, Serialize};
use serde_json_any_key::*;

#[derive(Serialize, Deserialize)]
pub enum HarvesterTarget {
    Controller(ObjectId<StructureController>), Spawn(ObjectId<StructureSpawn>)
}

#[derive(Serialize, Deserialize)]
pub struct HarvesterData {
    pub harvesting: bool,
    pub target: Option<HarvesterTarget>
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

#[derive(Serialize, Deserialize)]
pub struct SourceDistribution {
    #[serde(with = "any_key_map")] 
    pub harvest_positions: HashMap<ObjectId<Source>, SourceData>,
    pub creep_assignments: HashMap<String, (Position, ObjectId<Source>)>
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
                    .map(|pos| (pos, HarvestPositionData { assigned: HashSet::new(), capacity: 2 }))
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

pub fn do_harvester_creep(creep: &Creep, source_distribution: &mut SourceDistribution, data: &mut HarvesterData) -> Option<()> {
    if data.harvesting {
        if creep.store().get_free_capacity(None) == 0 {
            data.harvesting = false;
            data.target = None;
        }
    } else {
        if creep.store().get_used_capacity(None) == 0 {
            data.harvesting = true;
        }
    }

    if data.harvesting {
        if let Some((pos, source)) = source_distribution.get_assignmemnt(&creep) {
            let move_result = creep.move_to(pos);

            if creep.pos() == pos || move_result.is_err() {
                let source = source.resolve()?;
                creep.harvest(&source).ok();
            }
        } else {
            warn!("Creep {} has no assignment", creep.name())
        }
    } else {
        if data.target.is_none() {
            let room = creep.room()?;
            if room.energy_available() < room.energy_capacity_available() {
                data.target = Some(HarvesterTarget::Spawn(game::spawns().values().next()?.id()));
            } else {
                data.target = Some(HarvesterTarget::Controller(room.controller()?.id()));
            }
        }

        if let Some(target) = &data.target {
            match target {
                HarvesterTarget::Controller(target) => {
                    let target = target.resolve()?;
                    creep.move_to(&target).ok();

                    if creep.pos().is_near_to(target.pos()) {
                        creep.upgrade_controller(&target).ok();
                    }
                },
                HarvesterTarget::Spawn(target) => {
                    let target = target.resolve()?;
                    creep.move_to(&target).ok();

                    if creep.pos().is_near_to(target.pos()) {
                        creep.transfer(&target, ResourceType::Energy, None).ok();
                    }
                },
            }
        }
    }

    Some(())
}