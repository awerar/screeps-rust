use std::fmt::Debug;

use log::warn;
use screeps::{Creep, RoomName, Source, StructureSpawn, find, game, look, prelude::*};
use serde::{Deserialize, Serialize};

use crate::{creeps::{excavator::ExcavatorCreep, fabricator::FabricatorCreep, flagship::FlagshipCreep, truck::TruckCreep, tugboat::TugboatCreep}, id::{IDMaybeResolvable, IDMode, IDResolvable, Resolved, ResolvedId, Unresolved}, memory::Memory, movement::Movement, statemachine::StateMachineTransition, utils::adjacent_positions};

mod flagship;
mod excavator;
mod tugboat;
pub mod fabricator;
pub mod truck;

#[derive(Serialize, Deserialize)]
pub struct CreepData<M: IDMode> {
    pub role: CreepRole<M>,
    pub home: RoomName
}

impl CreepData<Resolved> {
    pub fn new(home: RoomName, role: CreepRole<Resolved>) -> Self {
        CreepData { role, home }
    }

    pub fn try_recover_from(creep: &Creep, mem: &Memory<Resolved>) -> Option<Self> {
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

                CreepRole::Excavator(ExcavatorCreep::default(), source.into()) 
            },
            _ => CreepRole::Scrap(get_recycle_spawn(creep, home.name).into())
        };
        
        Some(CreepData::new(home.name, role))
    }
}

impl IDMaybeResolvable for CreepData<Unresolved> {
    type Target = CreepData<Resolved>;

    fn try_id_resolve(self) -> Option<Self::Target> {
        Some(CreepData { role: self.role.try_id_resolve()?, home: self.home })
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum CreepRole<M: IDMode> {
    Excavator(ExcavatorCreep, M::Wrap<Source>),
    Flagship(FlagshipCreep<M>),
    Tugboat(TugboatCreep, M::Wrap<Creep>),
    Truck(TruckCreep),
    Fabricator(FabricatorCreep<M>),
    Scrap(M::Wrap<StructureSpawn>),
}

#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub enum CreepType {
    Excavator(ResolvedId<Source>), 
    Flagship,
    Tugboat(ResolvedId<Creep>),
    Truck,
    Fabricator,
    Scrap(ResolvedId<StructureSpawn>),
}

impl CreepRole<Resolved> {
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

impl IDMaybeResolvable for CreepRole<Unresolved> {
    type Target = CreepRole<Resolved>;

    fn try_id_resolve(self) -> Option<Self::Target> {
        Some(match self {
            Self::Excavator(state, source) => CreepRole::Excavator(state, source.try_id_resolve()?),
            Self::Flagship(state) => CreepRole::Flagship(state.id_resolve()),
            Self::Tugboat(state, tugged) => CreepRole::Tugboat(state, tugged.try_id_resolve()?),
            Self::Truck(state) => CreepRole::Truck(state),
            Self::Fabricator(state) => CreepRole::Fabricator(state.id_resolve()),
            Self::Scrap(controller) => CreepRole::Scrap(controller.try_id_resolve()?),
        })
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

    pub fn default_role(&self) -> CreepRole<Resolved> {
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

fn do_recycle(creep: &Creep, movement: &mut Movement<Resolved>, spawn: &ResolvedId<StructureSpawn>) {
    if creep.pos().is_near_to(spawn.pos()) {
        spawn.recycle_creep(creep).ok();
    } else {
        movement.smart_move_creep_to(creep, spawn.pos()).ok();
    }
}

pub fn do_creeps(mem: &mut Memory<Resolved>) {
    use CreepRole::*;

    let updatable_creeps: Vec<_> = game::creeps().values()
        .map(Into::<ResolvedId<Creep>>::into)
        .filter(|creep| !creep.spawning())
        .filter(|creep| {
            if !mem.creeps.contains_key(&creep) {
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
                    let mut args = (source.clone(), home, &mut mem.messages);
                    state.transition(creep, &mut args);
                },
                Tugboat(state, tugged) => {
                    let mut args = (home, tugged.clone(), &mut mem.movement, &mut mem.messages);
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