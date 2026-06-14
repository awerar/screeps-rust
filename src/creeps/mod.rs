use std::{collections::HashMap, fmt::Debug};

use derive_deref::{Deref, DerefMut};
use log::warn;
use screeps::{Creep, RoomName, Source, StructureSpawn, find, game, look, prelude::*};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::{colony::{ColonyView, planning::planned_ref::ResolvableStructureRef}, creeps::{excavator::ExcavatorCreep, fabricator::FabricatorCreep, flagship::FlagshipCreep, truck::{CreepStops, TruckCreep}}, memory::Memory, movement::requests::MovementRequests, safeid::{DO, GetSafeID, IDKind, MakeSafe, SafeID, SafeIDs, TryFromUnsafe, TryMakeSafe, UnsafeIDs, deserialize_prune_hashmap}, spawn::TugboatRequests, statemachine::transition, utils::adjacent_positions};

pub mod flagship;
pub mod excavator;
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
    Tugboat(I::ID<Creep>, I::ID<StructureSpawn>),
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
            Self::Unsafe::Tugboat(tugged, spawn) => Self::Tugboat(tugged.try_make_safe()?, spawn.try_make_safe()?),
            Self::Unsafe::Scrap(state) => Self::Scrap(state.try_make_safe()?),
        })
    }
}

impl CreepRole {
    pub fn prefix(&self) -> &str {
        match self {
            CreepRole::Flagship(_) => "Flagship",
            CreepRole::Excavator(_, _) => "Excavator",
            CreepRole::Tugboat(_, _) => "Tugboat",
            CreepRole::Scrap(_) => "Scrap",
            CreepRole::Truck(_) => "Truck",
            CreepRole::Fabricator(_) => "Fabricator",
        }
    }
}

fn do_recycle(creep: &SafeID<Creep>, movement: &mut MovementRequests, spawn: &SafeID<StructureSpawn>) {
    movement.move_creep_to(creep, spawn.pos(), 1);
    if creep.pos().is_near_to(spawn.pos()) {
        spawn.recycle_creep(creep).ok();
    }
}

pub fn do_creeps(mem: &mut Memory) -> TugboatRequests {
    use CreepRole::*;

    let update_creeps: Vec<_> = SafeID::creeps()
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

    let mut movement = MovementRequests::new();
    for creep in &update_creeps {
        let creep_data = mem.creeps.get_mut(creep).unwrap();
        let Some(home) = mem.colonies.view(creep_data.home) else { continue; };

        match &mut creep_data.role {
            Flagship(state) => 
                transition(state, |state| state.update(creep, &mut movement, &mut mem.claim_requests)),
            Excavator(state, source) => 
                transition(state, |state| state.update(creep, source, &home, &mut movement)),
            Truck(state) => 
                transition(state, |state| state.update(creep, &home, &mut movement, mem.truck_coordinators.entry(creep_data.home).or_default(), &mut false, &mut 0)),
            Fabricator(state) => 
                transition(state, |state| state.update(creep, &home, &mut movement, mem.fabricator_coordinators.entry(creep_data.home).or_default())),
            Tugboat(tugged, spawn) => movement.do_tugboat(creep, tugged, spawn),
            Scrap(spawn) => do_recycle(creep, &mut movement, spawn),
        }
    }

    movement.perform(&mut mem.movement)
}

impl Memory {
    pub fn get_creep_stops(&self, room: RoomName) -> CreepStops {
        let mut result = CreepStops { consumers: Vec::new(), providers: Vec::new() };

        for (creep, data) in &self.creeps.0 {
            if creep.pos().room_name() != room { continue; }
            let CreepRole::Fabricator(state) = &data.role else { continue; };

            if state.is_consumer() { result.consumers.push(creep.clone()); }
            if state.is_provider() { result.providers.push(creep.clone()); }
        }

        result
    }
}

fn get_recycle_spawn(creep: &Creep, home: &ColonyView<'_>) -> StructureSpawn {
    if creep.pos().room_name() == home.name
        && let Some(spawn) = creep.pos().find_closest_by_path(find::MY_SPAWNS, None) {
            return spawn
        }

    if let Some(spawn) = home.plan.center.spawn.resolve() {
        return spawn
    }

    game::spawns().values()
        .min_by_key(|spawn| creep.pos().get_range_to(spawn.pos()))
        .unwrap()
}