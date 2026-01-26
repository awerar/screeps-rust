use std::{collections::{HashMap, HashSet}, ops::Add};

use itertools::Itertools;
use log::*;
use screeps::{
    Position, ResourceType, Room, StructureController, StructureSpawn, Terrain, find, game, local::ObjectId, objects::{Creep, Source}, prelude::*
};
use serde::{Deserialize, Serialize};
use serde_json_any_key::*;

type HarvestAssignment = (Position, ObjectId<Source>);

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

#[derive(Serialize, Deserialize, Clone)]
pub enum HarvesterState {
    Idle,
    Harvesting(HarvestAssignment),
    Distributing(DistributionTarget)
}

#[derive(Serialize, Deserialize, Clone)]
pub enum DistributionTarget {
    Controller(ObjectId<StructureController>), Spawn(ObjectId<StructureSpawn>)
}

fn get_distribution_target(creep: &Creep) -> Option<DistributionTarget> {
    let room = creep.room()?;
    if room.energy_available() < room.energy_capacity_available() {
        Some(DistributionTarget::Spawn(game::spawns().values().next()?.id()))
    } else {
        Some(DistributionTarget::Controller(room.controller()?.id()))
    }
}

fn is_full(creep: &Creep) -> bool {
    creep.store().get_free_capacity(None) == 0
}

fn is_empty(creep: &Creep) -> bool {
    creep.store().get_used_capacity(None) == 0
}

fn get_idle_state_transition(creep: &Creep, source_distribution: &mut SourceDistribution) -> HarvesterState {
    if !is_empty(creep) {
        if let Some(target) = get_distribution_target(creep) {
            return HarvesterState::Distributing(target)
        }
    }

    if !is_full(creep) {
        if let Some(assignment) = source_distribution.get_assignmemnt(creep) {
            return HarvesterState::Harvesting(assignment)
        } else {
            warn!("{} has no harvest assignment. Idling.", creep.name());
        }
    }

    HarvesterState::Idle
}

pub fn do_harvester_creep(creep: &Creep, curr_state: HarvesterState, source_distribution: &mut SourceDistribution) -> Option<HarvesterState> {
    match &curr_state {
        HarvesterState::Idle => Some(get_idle_state_transition(creep, source_distribution)),
        HarvesterState::Harvesting((pos, source)) => {
            creep.move_to(*pos).ok();
            if creep.pos().is_near_to(*pos) {
                let source = source.resolve()?;
                creep.harvest(&source).ok();
            }

            if is_full(creep) { Some(get_idle_state_transition(creep, source_distribution)) }
            else { Some(curr_state) }
        },
        HarvesterState::Distributing(target) => {
            match target {
                DistributionTarget::Controller(target) => {
                    let target = target.resolve()?;
                    creep.move_to(&target).ok();

                    if creep.pos().is_near_to(target.pos()) {
                        creep.upgrade_controller(&target).ok();
                    }

                    if is_empty(creep) { Some(get_idle_state_transition(creep, source_distribution)) }
                    else { Some(curr_state) }
                },
                DistributionTarget::Spawn(target) => {
                    let target = target.resolve()?;
                    creep.move_to(&target).ok();

                    if creep.pos().is_near_to(target.pos()) {
                        creep.transfer(&target, ResourceType::Energy, None).ok();
                    }

                    Some(get_idle_state_transition(creep, source_distribution))
                },
            }
        },
    }
}