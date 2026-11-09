use std::fmt::Debug;

use derive_where::derive_where;
use log::{error, warn};
use screeps::{Creep, ResourceType, RoomName, Source, find, game, look, prelude::*};
use anyhow::Result;

use crate::{check::{Check, CheckFrom}, creeps::{excavator::ExcavatorCreep, fabricator::FabricatorCreep, flagship::FlagshipCreep, scrap::ScrapCreep, seeder::SeederCreep, servicer::ServicerCreep, terraformer::TerraformerCreep, truck::{CreepStops, ImportTruckState, TruckCreep}, virtual_creep::VirtualCreep}, domain_traits::{CreepId, EnergyStoreAccessors, HasId, ObjectId, ResolvableId, Withdrawable}, ids::{CheckState, Checked, Unchecked}, memory::Memory, movement::requests::{MovementRequests, TugboatRequests}, statemachine::{Wait, step, wait_if}, utils::adjacent_positions};

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
            "ImportTruck" => CreepRole::ImportTruck(if creep.used_energy_capacity() == 0 { ImportTruckState::default() } else { ImportTruckState::GoingHome }),
            "Fabricator" => CreepRole::Fabricator(FabricatorCreep::default()),
            "Servicer" => CreepRole::Servicer(ServicerCreep::default()),
            "Terraformer" => CreepRole::Terraformer(TerraformerCreep::default()),
            "Seeder" => CreepRole::Seeder(SeederCreep::default()),
            "Excavator" => {
                let source = adjacent_positions(creep.pos())
                    .flat_map(|pos| pos.look_for(look::SOURCES))
                    .flatten()
                    .next()
                    .or_else(|| creep.pos().find_closest_by_path(find::SOURCES, None))?;

                CreepRole::Excavator(ExcavatorCreep::default(), source.id()) 
            },
            _ => CreepRole::Scrap(ScrapCreep::default())
        };
        
        Some(CreepData::new(home.name, home.name, role))
    }
}

#[derive(Debug)]
#[derive_where(Serialize, Deserialize, Clone; ObjectId<Source, S>, CreepId<S>)]
pub enum CreepRole<S: CheckState = Checked> {
    Excavator(ExcavatorCreep, ObjectId<Source, S>),
    Flagship(FlagshipCreep),
    Truck(TruckCreep),
    ImportTruck(ImportTruckState),
    Fabricator(FabricatorCreep),
    Servicer(ServicerCreep),
    Tugboat(CreepId<S>),
    Terraformer(TerraformerCreep),
    Seeder(SeederCreep),
    Scrap(ScrapCreep),
}

impl CheckFrom for CreepRole {
    type Unchecked = CreepRole<Unchecked>;
    type Err = anyhow::Error;

    fn check_from(us: Self::Unchecked) -> Result<Self> {
        Ok(match us {
            Self::Unchecked::Excavator(state, source) => Self::Excavator(state, source.check()?),
            Self::Unchecked::Flagship(state) => Self::Flagship(state),
            Self::Unchecked::Truck(state) => Self::Truck(state),
            Self::Unchecked::ImportTruck(state) => Self::ImportTruck(state),
            Self::Unchecked::Fabricator(state) => Self::Fabricator(state),
            Self::Unchecked::Servicer(state) => Self::Servicer(state),
            Self::Unchecked::Tugboat(tugged) => Self::Tugboat(tugged.check()?),
            Self::Unchecked::Scrap(state) => Self::Scrap(state),
            Self::Unchecked::Terraformer(state) => Self::Terraformer(state),
            Self::Unchecked::Seeder(state) => Self::Seeder(state)
        })
    }
}

impl CreepRole {
    pub fn prefix(&self) -> &str {
        match self {
            CreepRole::Flagship(_) => "Flagship",
            CreepRole::Excavator(_, _) => "Excavator",
            CreepRole::Tugboat(_) => "Tugboat",
            CreepRole::Scrap(_) => "Scrap",
            CreepRole::Truck(_) => "Truck",
            CreepRole::ImportTruck(_) => "ImportTruck",
            CreepRole::Fabricator(_) => "Fabricator",
            CreepRole::Servicer(_) => "Servicer",
            CreepRole::Terraformer(_) => "Terraformer",
            CreepRole::Seeder(_) => "Seeder"
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

        match &mut creep_data.role {
            Flagship(state) => 
                step(state, |state| state.update(&mut vcreep, &mut movement, &mut mem.flagship_coordinator)),
            Excavator(state, source) => 
                step(state, |state| state.update(&mut vcreep, &source.resolve(), &home, &mut movement)),
            Truck(state) => {
                let coordinator = mem.truck_coordinators.entry(creep_data.home).or_default();
                step(state, |state| state.update(&mut vcreep, &home, &mut movement, coordinator));
            },
            ImportTruck(state) => {
                let coordinator = mem.truck_coordinators.entry(creep_data.home).or_default();
                step(state, |state| state.update(&mut vcreep, &home, &mem.colonies.view_export_colonies(), &mut movement, coordinator));
            }
            Fabricator(state) => {
                let coordinator = mem.fabricator_coordinators.entry(creep_data.home).or_default();
                step(state, |state| state.update(&mut vcreep, &home, &mut movement, coordinator));
            },
            Servicer(state) => {
                let coordinator = mem.servicer_coordinators.entry(creep_data.home).or_default();
                step(state, |state| state.update(&mut vcreep, &home, &mut movement, coordinator));
            }
            Terraformer(state) => {
                step(state, |state| state.update(&mut vcreep, &home, &mut movement));
            },
            Seeder(state) => {
                step(state, |state| state.update(&mut vcreep, &home, &mut movement));
            },
            Tugboat(tugged) => {
                let Some(recycle_spawn) = home.plan.center.spawn.resolve() else { continue; };
                movement.do_tugboat(creep, tugged.clone(), &recycle_spawn);
            },
            Scrap(state) => {
                step(state, |state| state.update(&mut vcreep, &home, &mut movement));
            },
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

        for (creep, data) in &self.creeps {
            if creep.resolve().spawning() { continue; }
            if creep.resolve().pos().room_name() != room { continue; }

            if matches!(data.role, CreepRole::Fabricator(_) | CreepRole::Servicer(_)) {
                result.consumers.push(creep.resolve());
            }
        }

        result
    }
}

pub fn gather_energy<P, F>(creep: &mut VirtualCreep, movement: &mut MovementRequests, find_structure: F) -> Wait 
    where
        P : Withdrawable + HasPosition + 'static,
        F : FnOnce() -> Wait<P>
{
    if creep.next_used_energy_capacity() > 0 { return Wait::Ready(()); }

    let structure = find_structure()?;

    movement.approach(creep, structure.pos(), 1)?;

    wait_if(structure.used_energy_capacity() == 0)?;
    wait_if(creep.outgoing() > 0)?;
    
    creep.withdraw(structure, ResourceType::Energy, None)?;

    Wait::Ready(())
}