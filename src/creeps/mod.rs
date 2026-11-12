use std::fmt::Debug;

use derive_where::derive_where;
use log::{error, warn};
use screeps::{Creep, ResourceType, RoomName, Source, find, game, look, prelude::*};
use anyhow::{Result, anyhow};

use crate::{check::{Check, CheckFrom}, colony::ColonyView, creeps::{fabricator::FabricatorCreep, flagship::FlagshipCreep, scrap::ScrapCreep, servicer::ServicerCreep, truck::{CreepStops, ExportTruckCreep, TruckCreep}, virtual_creep::VirtualCreep}, domain_traits::{CreepId, EnergyStoreAccessors, HasId, ObjectId, ResolvableId, Withdrawable}, ids::{CheckState, Checked, Unchecked}, memory::Memory, movement::requests::{MovementRequests, TugboatRequests}, statemachine::{Flow, break_if, run}, utils::adjacent_positions};

pub mod body;
pub mod flagship;
pub mod excavator;
pub mod truck;
pub mod terraformer;
pub mod seeder;
pub mod virtual_creep;
pub mod servicer;
pub mod fabricator;
pub mod scrap;

#[derive(Debug)]
#[derive_where(Deserialize, Serialize, Clone; CreepRole<S>)]
pub struct CreepData<S: CheckState = Checked> {
    pub role: CreepRole<S>,
    pub home: RoomName,
    pub spawn_room: RoomName
}

impl CheckFrom for CreepData {
    type Unchecked = CreepData<Unchecked>;
    type Err = anyhow::Error;

    fn check_from(us: Self::Unchecked) -> Result<Self> {
        Ok(Self {
            role: us.role.check()?,
            home: us.home,
            spawn_room: us.spawn_room
        })
    }
}

impl CreepData {
    pub fn new(home: RoomName, spawn_room: RoomName, role: CreepRole) -> Self {
        CreepData { role, home, spawn_room }
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
            "Servicer" => CreepRole::Servicer(ServicerCreep::default()),
            "Terraformer" => CreepRole::Terraformer,
            "Seeder" => CreepRole::Seeder,
            "Excavator" => {
                let source = adjacent_positions(creep.pos())
                    .flat_map(|pos| pos.look_for(look::SOURCES))
                    .flatten()
                    .next()
                    .or_else(|| creep.pos().find_closest_by_path(find::SOURCES, None))?;

                CreepRole::Excavator(source.id())
            },
            _ => CreepRole::Scrap(ScrapCreep::default())
        };
        
        Some(CreepData::new(home.name, home.name, role))
    }
}

#[derive(Debug)]
#[derive_where(Serialize, Deserialize, Clone; ObjectId<Source, S>, CreepId<S>)]
pub enum CreepRole<S: CheckState = Checked> {
    Excavator(ObjectId<Source, S>),
    Flagship(FlagshipCreep),
    Truck(TruckCreep),
    ExportTruck(ExportTruckCreep),
    Fabricator(FabricatorCreep),
    Servicer(ServicerCreep),
    Tugboat(CreepId<S>),
    Terraformer,
    Seeder,
    Scrap(ScrapCreep),
}

impl CheckFrom for CreepRole {
    type Unchecked = CreepRole<Unchecked>;
    type Err = anyhow::Error;

    fn check_from(us: Self::Unchecked) -> Result<Self> {
        Ok(match us {
            Self::Unchecked::Excavator(state) => Self::Excavator(state.check()?),
            Self::Unchecked::Flagship(state) => Self::Flagship(state),
            Self::Unchecked::Truck(state) => Self::Truck(state),
            Self::Unchecked::ExportTruck(state) => Self::ExportTruck(state),
            Self::Unchecked::Fabricator(state) => Self::Fabricator(state),
            Self::Unchecked::Servicer(state) => Self::Servicer(state),
            Self::Unchecked::Tugboat(tugged) => Self::Tugboat(tugged.check()?),
            Self::Unchecked::Scrap(state) => Self::Scrap(state),
            Self::Unchecked::Terraformer => Self::Terraformer,
            Self::Unchecked::Seeder => Self::Seeder
        })
    }
}

impl CreepRole {
    pub fn prefix(&self) -> &str {
        match self {
            CreepRole::Flagship(_) => "Flagship",
            CreepRole::Excavator(_) => "Excavator",
            CreepRole::Tugboat(_) => "Tugboat",
            CreepRole::Scrap(_) => "Scrap",
            CreepRole::Truck(_) => "Truck",
            CreepRole::ExportTruck(_) => "ExportTruck",
            CreepRole::Fabricator(_) => "Fabricator",
            CreepRole::Servicer(_) => "Servicer",
            CreepRole::Terraformer => "Terraformer",
            CreepRole::Seeder => "Seeder"
        }
    }
}

pub fn do_creeps(mem: &mut Memory) -> TugboatRequests {
    use CreepRole::*;

    let update_creeps: Vec<_> = game::creeps().values()
        .filter(|creep| !creep.spawning())
        .filter(|creep| {
            if !mem.creeps.contains_key(&creep.id()) {
                let Some(config) = CreepData::try_recover_from(creep, mem) else {
                    warn!("Unable to recover creep data for {}", creep.name());
                    creep.suicide().unwrap();
                    return false;
                };

                mem.creeps.insert(creep.id(), config);
            }

            true
        }).collect();

    let mut movement = MovementRequests::new();
    for creep in &update_creeps {
        let creep_data = mem.creeps.get_mut(&creep.id()).unwrap();
        let Some(home) = mem.colonies.view(creep_data.home) else { continue; };

        let mut vcreep = VirtualCreep::new(creep.clone());

        let result = match &mut creep_data.role {
            Flagship(state) => state.update(&mut vcreep, &mut mem.flagship_coordinator),
            Excavator(source) => excavator::update(&mut vcreep, &source.resolve(), &home).into_result(),
            Truck(state) => {
                let coordinator = mem.truck_coordinators.entry(creep_data.home).or_default();
                state.update(&mut vcreep, &home, coordinator)
            },
            ExportTruck(state) => {
                if let Some(import_colony) = mem.colonies.view(state.import_colony()) {
                    let import_coordinator = mem.truck_coordinators.entry(state.import_colony()).or_default();

                    state.update(&mut vcreep, &home, &import_colony, import_coordinator)
                } else {
                    Err(anyhow!("Import colony is no longer a colony"))
                }
            }
            Fabricator(state) => {
                let coordinator = mem.fabricator_coordinators.entry(creep_data.home).or_default();
                state.update(&mut vcreep, &home, coordinator)
            },
            Servicer(state) => {
                let coordinator = mem.servicer_coordinators.entry(creep_data.home).or_default();
                state.update(&mut vcreep, &home, coordinator)
            }
            Terraformer => terraformer::update(&mut vcreep, &home).into_result(),
            Seeder => run(|| seeder::transition(&mut vcreep, &home)),
            Tugboat(tugged) => {
                let Some(recycle_spawn) = home.plan.center.spawn.resolve() else { continue; };
                movement.do_tugboat(creep, tugged.clone(), &recycle_spawn);
                Ok(())
            },
            Scrap(state) => state.update(&mut vcreep, &home),
        };

        if let Err(e) = result {
            error!("{} update failed: {e:#}", creep.name());
        }

        if let Err(e) = vcreep.commit(&mut movement) {
            error!("Failed to comit intents for {}: {}", creep.name(), e);
        }
    }

    movement.perform(&mut mem.movement)
}

impl Memory {
    pub fn get_creep_stops(&self, colony: &ColonyView<'_>) -> CreepStops {
        let mut result = CreepStops { consumers: Vec::new(), providers: Vec::new() };

        for (creep, data) in &self.creeps {
            if creep.resolve().spawning() { continue; }
            if creep.resolve().pos().room_name() != colony.name { continue; }

            let is_consumer = match data.role {
                CreepRole::Fabricator(_) | CreepRole::Servicer(_) => true,
                CreepRole::Seeder | CreepRole::Terraformer => !colony.plan.upgrader_container.is_complete(),
                _ => false,
            };
            
            if is_consumer {
                result.consumers.push(creep.resolve());
            }
        }

        result
    }
}

pub fn gather_energy<P, F>(creep: &mut VirtualCreep, find_structure: F) -> Flow 
    where
        P : Withdrawable + HasPosition + 'static,
        F : FnOnce() -> Flow<P>
{
    if creep.next_used_energy_capacity() > 0 { return Flow::Continue(()); }

    let structure = find_structure()?;

    creep.approach(structure.pos(), 1)?;

    break_if(structure.used_energy_capacity() == 0)?;
    break_if(creep.outgoing() > 0)?;
    
    creep.withdraw(structure, ResourceType::Energy, None)?;

    Flow::Continue(())
}