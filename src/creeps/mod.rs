use std::fmt::Debug;

use log::warn;
use screeps::{Creep, RoomName, Source, StructureSpawn, find, game, look, prelude::*};
use serde::{Deserialize, Serialize};

use crate::{checked_id::{CheckedID, CreepGetCheckedID, GetCheckedID, TryCheckIDs}, creeps::{excavator::ExcavatorCreep, fabricator::FabricatorCreep, flagship::FlagshipCreep, truck::TruckCreep, tugboat::TugboatCreep}, memory::Memory, movement::Movement, statemachine::StateMachineTransition, utils::adjacent_positions};

mod flagship;
mod excavator;
mod tugboat;
pub mod fabricator;
pub mod truck;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CreepData {
    pub role: CreepRole,
    pub home: RoomName
}

impl CreepData {
    pub fn new(home: RoomName, role: CreepRole) -> Self {
        CreepData { role, home }
    }

    pub fn try_recover_from(creep: &Creep, mem: &Memory) -> Option<Self> {
        let home = mem.colonies.view(creep.pos().room_name())
            .filter(|colony| colony.plan.center.spawn.is_complete())
            .or_else(|| 
                mem.colonies.view_all()
                    .filter(|colony| colony.plan.center.spawn.is_complete())
                    .min_by_key(|colony| colony.plan.center.pos.get_range_to(creep.pos()))
            )?;

        let role = match creep.name().split_ascii_whitespace().next()? {
            "Flagship" => CreepRole::Flagship(FlagshipCreep::default()),
            "Truck" => CreepRole::Truck(TruckCreep::default()),
            "Fabricator" => CreepRole::Fabricator(FabricatorCreep::default()),
            "Excavator" => {
                let source = adjacent_positions(creep.pos())
                    .flat_map(|pos| pos.look_for(look::SOURCES))
                    .flatten()
                    .next()
                    .or_else(|| creep.pos().find_closest_by_path(find::SOURCES, None))?;

                CreepRole::Excavator(ExcavatorCreep::default(), source.checked_id()) 
            },
            _ => CreepRole::Scrap(get_recycle_spawn(creep, home.name).checked_id())
        };
        
        Some(CreepData::new(home.name, role))
    }
}

impl TryCheckIDs for CreepData {
    fn try_check_ids(mut self) -> Option<Self> {
        self.role = self.role.try_check_ids()?;
        Some(self)
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum CreepRole {
    Excavator(ExcavatorCreep, CheckedID<Source>),
    Flagship(FlagshipCreep),
    Tugboat(TugboatCreep, CheckedID<Creep>),
    Truck(TruckCreep),
    Fabricator(FabricatorCreep),
    Scrap(CheckedID<StructureSpawn>),
}

impl TryCheckIDs for CreepRole {
    fn try_check_ids(self) -> Option<Self> {
        Some(match self {
            Self::Excavator(state, source) => Self::Excavator(state, source.try_check_ids()?),
            Self::Tugboat(state, tugged) => Self::Tugboat(state, tugged.try_check_ids()?),
            Self::Flagship(_) => self,
            Self::Truck(_) => self,
            Self::Fabricator(_) => self,
            Self::Scrap(_) => self,
        })
    }
}

#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub enum CreepType {
    Excavator(CheckedID<Source>), 
    Flagship,
    Tugboat(CheckedID<Creep>),
    Truck,
    Fabricator,
    Scrap(CheckedID<StructureSpawn>),
}

impl CreepRole {
    pub fn get_type(&self) -> CreepType {
        match self {
            CreepRole::Flagship(_) => CreepType::Flagship,
            CreepRole::Excavator(_, source) => CreepType::Excavator(source.clone()),
            CreepRole::Tugboat(_, tugged) => CreepType::Tugboat(tugged.clone()),
            CreepRole::Scrap(source) => CreepType::Scrap(source.clone()),
            CreepRole::Truck(_) => CreepType::Truck,
            CreepRole::Fabricator(_) => CreepType::Fabricator,
        }
    }
}

impl CreepType {
    pub fn prefix(&self) -> &str {
        match self {
            CreepType::Flagship => "Flagship",
            CreepType::Excavator(_) => "Excavator",
            CreepType::Tugboat(_) => "Tugboat",
            CreepType::Scrap(_) => "Scrap",
            CreepType::Truck => "Truck",
            CreepType::Fabricator => "Fabricator",
        }
    }

    pub fn default_role(&self) -> CreepRole {
        match self {
            CreepType::Flagship => CreepRole::Flagship(FlagshipCreep::default()),
            CreepType::Excavator(source) => CreepRole::Excavator(ExcavatorCreep::default(), source.clone()),
            CreepType::Tugboat(tugged) => CreepRole::Tugboat(TugboatCreep::default(), tugged.clone()),
            CreepType::Scrap(spawn) => CreepRole::Scrap(spawn.clone()),
            CreepType::Truck => CreepRole::Truck(TruckCreep::default()),
            CreepType::Fabricator => CreepRole::Fabricator(FabricatorCreep::default()),
        }
    }
}

fn do_recycle(creep: &Creep, movement: &mut Movement, spawn: &CheckedID<StructureSpawn>) {
    if creep.pos().is_near_to(spawn.pos()) {
        spawn.recycle_creep(creep).ok();
    } else {
        movement.smart_move_creep_to(creep, spawn.pos()).ok();
    }
}

pub fn do_creeps(mem: &mut Memory) {
    use CreepRole::*;

    let updatable_creeps: Vec<_> = game::creeps().values()
        .map(|creep| creep.checked_id())
        .filter(|creep| !creep.spawning())
        .filter(|creep| {
            if !mem.creeps.contains_key(creep) {
                let Some(config) = CreepData::try_recover_from(creep, mem) else {
                    warn!("Unable to recover creep data for {}", creep.name());
                    return false;
                };

                mem.creeps.insert(creep.clone(), config);
            }

            true
        }).collect();

    let mut update_creeps = updatable_creeps.clone();
    while !update_creeps.is_empty() {
        for creep in &update_creeps {
            let creep_data = mem.creeps.get_mut(creep).unwrap();
            let Some(home) = mem.colonies.view(creep_data.home) else { continue; };

            match &mut creep_data.role {
                Flagship(state) => {
                    let mut args  = (&mut mem.movement, &mut mem.claim_requests);
                    state.transition(creep, &mut args);
                },
                Excavator(state, source) => {
                    let mut args = (source.id, home, &mut mem.messages);
                    state.transition(creep, &mut args);
                },
                Tugboat(state, tugged) => {
                    let mut args = (home, tugged.id, &mut mem.movement, &mut mem.messages);
                    state.transition(creep, &mut args);
                },
                Scrap(spawn) => do_recycle(creep, &mut mem.movement, spawn),
                Truck(state) => {
                    let mut args = (home, &mut mem.movement, mem.truck_coordinators.entry(creep_data.home).or_default(), &mut mem.messages);
                    state.transition(creep, &mut args);
                },
                Fabricator(state) => {
                    let mut args = (home, &mut mem.movement, mem.fabricator_coordinators.entry(creep_data.home).or_default(), &mut mem.messages);
                    state.transition(creep, &mut args);
                }
            }
        }

        for creep in &updatable_creeps {
            mem.messages.creep_quick(creep).flush();
        }

        update_creeps = updatable_creeps.iter()
            .filter(|creep| !mem.messages.creep_quick(creep).empty())
            .cloned()
            .collect();
    }

    for creep in &updatable_creeps {
        mem.messages.creep(creep).flush();
    }
}

fn get_recycle_spawn(creep: &Creep, home: RoomName) -> StructureSpawn {
    if creep.pos().room_name() == home {
        if let Some(spawn) = creep.pos().find_closest_by_path(find::MY_SPAWNS, None) {
            return spawn
        }
    }

    if let Some(home) = game::rooms().get(home) {
        if let Some(spawn) = home.find(find::MY_SPAWNS, None).into_iter().next() {
            return spawn
        }
    }

    game::spawns().values()
        .min_by_key(|spawn| creep.pos().get_range_to(spawn.pos()))
        .unwrap()
}