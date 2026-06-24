use std::{collections::HashMap, fmt::Debug, mem};

use derive_deref::{Deref, DerefMut};
use derive_where::derive_where;
use log::{error, warn};
use screeps::{Creep, RoomName, Source, StructureSpawn, find, game, look, prelude::*};
use serde::{Deserialize, Serialize};

use crate::{check::{Check, CheckFrom, deserialize_filter_check}, colony::{ColonyView, planning::planned_ref::ResolvableStructureRef}, creeps::{excavator::ExcavatorCreep, fabricator::FabricatorCreep, flagship::FlagshipCreep, truck::{CreepStops, TruckCreep}, virtual_creep::VirtualCreep}, ids::{ById, CheckState, Checked, Unchecked, WithId}, memory::Memory, movement::requests::MovementRequests, spawn::TugboatRequests, statemachine::transition, utils::adjacent_positions};

pub mod flagship;
pub mod excavator;
pub mod fabricator;
pub mod truck;
pub mod virtual_creep;

#[derive(Default, Deserialize, Serialize, Deref, DerefMut)]
pub struct Creeps(
    #[serde(deserialize_with = "deserialize_filter_check")]
    pub HashMap<WithId<Creep>, CreepData>
);

#[derive(Debug)]
#[derive_where(Deserialize, Serialize, Clone; CreepRole<S>)]
pub struct CreepData<S: CheckState = Checked> {
    pub role: CreepRole<S>,
    pub home: RoomName
}

impl CheckFrom for CreepData {
    type Unchecked = CreepData<Unchecked>;
    type Err = ();

    fn check_from(us: Self::Unchecked) -> Result<Self, ()> {
        Ok(Self {
            role: us.role.check()?,
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

                CreepRole::Excavator(ExcavatorCreep::default(), ById(source)) 
            },
            _ => CreepRole::Scrap(ById(get_recycle_spawn(creep, &home)))
        };
        
        Some(CreepData::new(home.name, role))
    }
}

#[derive(Debug)]
#[derive_where(Serialize, Deserialize, Clone; S::Repr<Source>, S::Repr<WithId<Creep>>, S::Repr<StructureSpawn>)]
pub enum CreepRole<S: CheckState = Checked> {
    Excavator(ExcavatorCreep, S::Repr<Source>),
    Flagship(FlagshipCreep),
    Truck(TruckCreep),
    Fabricator(FabricatorCreep),
    Tugboat(S::Repr<WithId<Creep>>, S::Repr<StructureSpawn>),
    Scrap(S::Repr<StructureSpawn>),
}

impl CheckFrom for CreepRole {
    type Unchecked = CreepRole<Unchecked>;
    type Err = ();

    fn check_from(us: Self::Unchecked) -> Result<Self, ()> {
        Ok(match us {
            Self::Unchecked::Excavator(state, source) => Self::Excavator(state, source.check()?),
            Self::Unchecked::Flagship(state) => Self::Flagship(state),
            Self::Unchecked::Truck(state) => Self::Truck(state),
            Self::Unchecked::Fabricator(state) => Self::Fabricator(state),
            Self::Unchecked::Tugboat(tugged, spawn) => Self::Tugboat(tugged.check()?, spawn.check()?),
            Self::Unchecked::Scrap(state) => Self::Scrap(state.check()?),
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

fn do_recycle(creep: &WithId<Creep>, movement: &mut MovementRequests, spawn: &StructureSpawn) {
    if movement.move_creep_to(creep, spawn.pos(), 1).in_range() {
        spawn.recycle_creep(creep).ok();
    }
}

pub fn do_creeps(mem: &mut Memory) -> TugboatRequests {
    use CreepRole::*;

    let update_creeps: Vec<_> = WithId::creeps()
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

        let mut vcreep = VirtualCreep::new(creep.clone());

        match &mut creep_data.role {
            Flagship(state) => 
                transition(state, |state| state.update(creep, &mut movement, &mut mem.claim_requests)),
            Excavator(state, source) => 
                transition(state, |state| state.update(&mut vcreep, source, &home, &mut movement)),
            Truck(state) => {
                let coordinator = mem.truck_coordinators.entry(creep_data.home).or_default();
                *state = mem::take(state).update(&mut vcreep, &home, &mut movement, coordinator);
            },
            Fabricator(state) => {
                let coordinator = mem.fabricator_coordinators.entry(creep_data.home).or_default();
                transition(state, |state| state.update(creep, &home, &mut movement, coordinator));
            },
            Tugboat(tugged, spawn) => movement.do_tugboat(creep, tugged, spawn),
            Scrap(spawn) => do_recycle(creep, &mut movement, spawn),
        }

        if let Err(e) = vcreep.commit() {
            error!("Failed to comit intents for {}: {}", creep.name(), e);
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