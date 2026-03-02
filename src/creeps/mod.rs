use std::{collections::HashMap, fmt::Debug};

use derive_deref::{Deref, DerefMut};
use log::warn;
use screeps::{Creep, RoomName, Source, StructureSpawn, find, game, look, prelude::*};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::{colony::{ColonyView, planning::planned_ref::ResolvableStructureRef}, creeps::{excavator::ExcavatorCreep, fabricator::FabricatorCreep, flagship::FlagshipCreep, truck::TruckCreep}, memory::Memory, movement::MovementSolver, safeid::{DO, GetSafeID, IDKind, MakeSafe, SafeID, SafeIDs, TryFromUnsafe, TryMakeSafe, UnsafeIDs, deserialize_prune_hashmap}, statemachine::StateMachineTransition, utils::adjacent_positions};

mod flagship;
mod excavator;
mod tugged;
pub mod fabricator;
pub mod truck;

#[derive(Default, Deserialize, Serialize, Deref, DerefMut)]
pub struct Creeps(
    #[serde(deserialize_with = "deserialize_prune_hashmap")]
    pub HashMap<SafeID<Creep>, CreepData>
);

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(bound(deserialize="CreepRole<I> : DeserializeOwned"))]
pub struct CreepData<I: IDKind = SafeIDs> {
    pub role: CreepRole<I>,
    pub home: RoomName
}

impl TryFromUnsafe for CreepData {
    type Unsafe = CreepData<UnsafeIDs>;

    fn try_from_unsafe(us: Self::Unsafe) -> Option<Self> {
        Some(Self {
            role: us.role.try_make_safe()?,
            home: us.home
        })
    }
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

                CreepRole::Excavator(ExcavatorCreep::default(), source.safe_id()) 
            },
            _ => CreepRole::Scrap(get_recycle_spawn(creep, &home).safe_id())
        };
        
        Some(CreepData::new(home.name, role))
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(bound(deserialize = "I::ID<Source> : DO, I::ID<Creep> : DO, I::ID<StructureSpawn> : DO"))]
pub enum CreepRole<I: IDKind = SafeIDs> {
    Excavator(ExcavatorCreep, I::ID<Source>),
    Flagship(FlagshipCreep),
    Truck(TruckCreep),
    Fabricator(FabricatorCreep),
    Tugboat(I::ID<Creep>),
    Scrap(I::ID<StructureSpawn>),
}

impl TryFromUnsafe for CreepRole {
    type Unsafe = CreepRole<UnsafeIDs>;

    fn try_from_unsafe(us: Self::Unsafe) -> Option<Self> {
        Some(match us {
            Self::Unsafe::Excavator(state, source) => Self::Excavator(state.make_safe(), source.try_make_safe()?),
            Self::Unsafe::Flagship(state) => Self::Flagship(state),
            Self::Unsafe::Truck(state) => Self::Truck(state),
            Self::Unsafe::Fabricator(state) => Self::Fabricator(state),
            Self::Unsafe::Tugboat(tugged) => Self::Tugboat(tugged.try_make_safe()?),
            Self::Unsafe::Scrap(state) => Self::Scrap(state.try_make_safe()?),
        })
    }
}

#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub enum CreepType {
    Excavator(SafeID<Source>), 
    Flagship,
    Tugboat(SafeID<Creep>),
    Truck,
    Fabricator,
    Scrap(SafeID<StructureSpawn>),
}

impl CreepRole {
    pub fn get_type(&self) -> CreepType {
        match self {
            CreepRole::Flagship(_) => CreepType::Flagship,
            CreepRole::Excavator(_, source) => CreepType::Excavator(source.clone()),
            CreepRole::Tugboat(tugged) => CreepType::Tugboat(tugged.clone()),
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
            CreepType::Tugboat(tugged) => CreepRole::Tugboat(tugged.clone()),
            CreepType::Scrap(spawn) => CreepRole::Scrap(spawn.clone()),
            CreepType::Truck => CreepRole::Truck(TruckCreep::default()),
            CreepType::Fabricator => CreepRole::Fabricator(FabricatorCreep::default()),
        }
    }
}

fn do_recycle(creep: &Creep, movement_solver: &mut MovementSolver, spawn: &SafeID<StructureSpawn>) {
    movement_solver.move_creep_to(creep, spawn.pos(), 1);
    if creep.pos().is_near_to(spawn.pos()) {
        spawn.recycle_creep(creep).ok();
    }
}

fn do_tugboat(tugboat: &Creep, tugged: &SafeID<Creep>, movement_solver: &mut MovementSolver, home: ColonyView<'_>) -> CreepRole {
    if movement_solver.move_tugboat(tugboat, tugged) {
        CreepRole::Scrap(get_recycle_spawn(tugboat, &home).safe_id())
    } else {
        CreepRole::Tugboat(tugged.clone())
    }
}

pub fn do_creeps(mem: &mut Memory) {
    use CreepRole::*;

    let update_creeps: Vec<_> = game::creeps().values()
        .map(|creep| creep.safe_id())
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

    let mut movement_solver = MovementSolver::new();
    for creep in &update_creeps {
        let creep_data = mem.creeps.get_mut(creep).unwrap();
        let Some(home) = mem.colonies.view(creep_data.home) else { continue; };

        match &mut creep_data.role {
            Flagship(state) => {
                let mut args  = (&mut movement_solver, &mut mem.claim_requests);
                state.transition(creep, &mut args);
            },
            Excavator(state, source) => {
                let mut args = (source.clone(), home, &mut mem.messages, &mut movement_solver);
                state.transition(creep, &mut args);
            },
            Truck(state) => {
                let mut args = (home, &mut movement_solver, mem.truck_coordinators.entry(creep_data.home).or_default(), &mut mem.messages);
                state.transition(creep, &mut args);
            },
            Fabricator(state) => {
                let mut args = (home, &mut movement_solver, mem.fabricator_coordinators.entry(creep_data.home).or_default(), &mut mem.messages);
                state.transition(creep, &mut args);
            },
            Tugboat(tugged) => creep_data.role = do_tugboat(creep, tugged, &mut movement_solver, home),
            Scrap(spawn) => do_recycle(creep, &mut movement_solver, &spawn),
        }
    }

    for creep in &update_creeps {
        mem.messages.creep(creep).flush();
    }
}

fn get_recycle_spawn(creep: &Creep, home: &ColonyView<'_>) -> StructureSpawn {
    if creep.pos().room_name() == home.name {
        if let Some(spawn) = creep.pos().find_closest_by_path(find::MY_SPAWNS, None) {
            return spawn
        }
    }

    if let Some(spawn) = home.plan.center.spawn.resolve() {
        return spawn
    }

    game::spawns().values()
        .min_by_key(|spawn| creep.pos().get_range_to(spawn.pos()))
        .unwrap()
}