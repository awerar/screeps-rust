use std::fmt::Debug;

use log::*;
use screeps::{Creep, ObjectId, RoomName, Source, find, game, look, prelude::*};
use serde::{Deserialize, Serialize};

use crate::{creeps::{claimer::ClaimerState, harvester::HarvesterState, remote_builder::RemoteBuilderState, tugboat::{TugboatState, TuggedState}, worker::WorkerState}, memory::Memory, utils::adjacent_positions};

mod claimer;
mod worker;
mod harvester;
mod remote_builder;
mod tugboat;

pub trait CreepState where Self : Sized + Default + Eq + Debug {
    fn update(&self, creep: &Creep, mem: &mut Memory) -> Result<Self, ()>;
}

fn transition<S>(state: &S, creep: &Creep, mem: &mut Memory) -> S where S : CreepState {
    let Ok(new_state) = state.update(creep, mem) else {
        if *state == S::default() {
            error!("{} failed on default state", creep.name());
            return S::default()
        } else {
            error!("{} failed on state {:?}. Falling back to default state", creep.name(), state);
            return S::default() // TODO: This should probably execute the default state
        }
    };

    if new_state != *state {
        transition(&new_state, creep, mem)
    } else {
        new_state
    }
}


#[derive(Serialize, Deserialize, Clone)]
pub struct CreepData {
    pub role: CreepRole,
    pub home: RoomName
}

impl CreepData {
    pub fn new(home: RoomName, role: CreepRole) -> Self {
        CreepData { role, home }
    }

    pub fn try_recover_from(creep: &Creep, mem: &Memory) -> Option<Self> {
        let home = mem.colony(creep.pos().room_name())
            .filter(|colony| colony.spawn().is_some())
            .or_else(|| 
                mem.colonies.values()
                .filter(|colony| colony.spawn().is_some())
                .min_by_key(|colony| colony.center.get_range_to(creep.pos()))
            )?;

        let role = match creep.name().split_ascii_whitespace().next()? {
            "Worker" => CreepRole::Worker(Default::default()),
            "Claimer" => CreepRole::Claimer(Default::default()),
            "RemoteBuilder" => CreepRole::RemoteBuilder(Default::default()),
            "Harvester" => {
                let source = adjacent_positions(creep.pos())
                    .flat_map(|pos| pos.look_for(look::SOURCES))
                    .flatten()
                    .next()
                    .or_else(|| creep.pos().find_closest_by_path(find::SOURCES, None))?;

                CreepRole::Harvester(Default::default(), source.id()) 
            },
            "Tugboat" => return None,
            _ => return None
        };
        
        Some(CreepData::new(home.room_name, role))
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum CreepRole {
    Worker(WorkerState),
    Harvester(HarvesterState, ObjectId<Source>),
    Claimer(ClaimerState),
    RemoteBuilder(RemoteBuilderState),
    Tugboat(TugboatState, ObjectId<Creep>)
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub enum CreepType {
    Worker,
    Harvester(ObjectId<Source>), 
    Claimer,
    RemoteBuilder,
    Tugboat(ObjectId<Creep>)
}

impl CreepRole {
    pub fn get_type(&self) -> CreepType {
        match self {
            CreepRole::Worker(_) => CreepType::Worker,
            CreepRole::Claimer(_) => CreepType::Claimer,
            CreepRole::RemoteBuilder(_) => CreepType::RemoteBuilder,
            CreepRole::Harvester(_, source) => CreepType::Harvester(*source),
            CreepRole::Tugboat(_, client) => CreepType::Tugboat(*client)
        }
    }

    pub fn tugged_state_mut(&mut self) -> Option<&mut TuggedState> {
        match self {
            CreepRole::Harvester(HarvesterState::Going(tugged_state), _) => Some(tugged_state),
            _ => None
        }
    }

    fn master(&self) -> Option<ObjectId<Creep>> {
        match self {
            CreepRole::Tugboat(TugboatState::Tugging, owner) => Some(owner.clone()),
            _ => None
        }
    }

    fn fallback(&self) -> Self {
        use CreepRole::*;

        match self {
            Worker(_) => Worker(Default::default()),
            Harvester(_, source) => Harvester(Default::default(), *source),
            Claimer(_) => Claimer(Default::default()),
            RemoteBuilder(_) => RemoteBuilder(Default::default()),
            Tugboat(_, tugged) => Tugboat(Default::default(), *tugged),
        }
    }
}

impl CreepType {
    pub fn prefix(&self) -> &str {
        match self {
            CreepType::Worker => "Worker",
            CreepType::Claimer => "Claimer",
            CreepType::RemoteBuilder => "RemoteBuilder",
            CreepType::Harvester(_) => "Harvester",
            CreepType::Tugboat(_) => "Tugboat"
        }
    }

    pub fn default_role(&self) -> CreepRole {
        match self {
            CreepType::Worker => CreepRole::Worker(Default::default()),
            CreepType::Claimer => CreepRole::Claimer(Default::default()),
            CreepType::RemoteBuilder => CreepRole::RemoteBuilder(Default::default()),
            CreepType::Harvester(source) => CreepRole::Harvester(Default::default(), *source),
            CreepType::Tugboat(client) => CreepRole::Tugboat(Default::default(), *client)
        }
    }
}

pub fn do_creeps(mem: &mut Memory) {
    use CreepRole::*;

    for creep in game::creeps().values() {
        if !mem.creeps.contains_key(&creep.name()) {
            let Some(config) = CreepData::try_recover_from(&creep, mem) else {
                warn!("Unable to recover creep data for {}", creep.name());
                continue;
            };

            mem.creeps.insert(creep.name(), config);
        }
    }

    let creeps: Vec<_> = mem.creeps.iter()
        .map(|(name, data)| (name.clone(), data.role.clone()))
        .collect();

    for (name, mut role) in creeps {
        let Some(creep) = game::creeps().get(name) else { continue; };

        if let Some(master) = role.master() {
            if master.resolve().is_some() {
                continue;
            } else {
                warn!("Master of {} dissapeared. Falling back", creep.name());
                role = role.fallback();
            }
        }

        role = match &role {
            Worker(state) => Worker(transition(&state, &creep, mem)),
            Claimer(state) => Claimer(transition(&state, &creep, mem)),
            RemoteBuilder(state) => RemoteBuilder(transition(&state, &creep, mem)),
            Harvester(state, source) => Harvester(transition(&state, &creep, mem), *source),
            Tugboat(state, client) => Tugboat(transition(&state, &creep, mem), *client)
        };

        mem.creeps.get_mut(&creep.name()).unwrap().role = role;
    }
}