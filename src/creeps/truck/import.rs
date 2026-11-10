use std::{collections::HashMap, fmt::Display, ops::Deref};

use derive_deref::Deref;
use derive_where::derive_where;
use enum_display::EnumDisplay;
use screeps::{HasPosition, ResourceType, RoomName};
use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::{check::Check, colony::{Colonies, ColonyBuffer, ColonyView, steps::ColonyStep}, creeps::{truck::{TruckCoordinator, stop::ConsumerTruckStop}, virtual_creep::VirtualCreep}, domain_traits::EnergyStoreAccessors, ids::{CheckState, Checked, Unchecked}, next, next_if, statemachine::{Step, step}};

pub const STOP_IMPORT_STEP: ColonyStep = ColonyStep::UpgradeToLevel5;
const START_EXPORT_STEP: ColonyStep = ColonyStep::UpgradeToLevel6;

#[derive(Deserialize, Serialize, Clone, Debug, Default)]
pub struct ImportTruckCreep {
    state: State,

    total_trip_ticks: u32,
    trips: u32
}

impl Display for ImportTruckCreep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} [{:?} rtt]", self.state, self.roundtrip_ticks())
    }
}

#[derive(Deserialize, Serialize, Clone, Debug, Default, EnumDisplay)]
pub enum State {
    #[default] Idle,
    CollectingFrom(RoomName),
    GoingHome(u32),
    Arrived(ArrivedState)
}

#[derive(Debug, Default, EnumDisplay)]
#[derive_where(Serialize, Deserialize, Clone; ConsumerTruckStop<S>, ColonyBuffer<S>, S)]
pub enum ArrivedState<S: CheckState = Checked> {
    #[default] Idle,
    ProvidingTo(ConsumerTruckStop<S>),
    StoringAway(ColonyBuffer<S>)
}

impl<'de> Deserialize<'de> for ArrivedState {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let us = ArrivedState::<Unchecked>::deserialize(deserializer)?;
        Ok(match us {
            ArrivedState::Idle => Self::Idle,
            ArrivedState::ProvidingTo(consumer) => 
                consumer.check().map_or_default(Self::ProvidingTo),
            ArrivedState::StoringAway(buffer) => 
                buffer.check().map_or_default(Self::StoringAway)
        })
    }
}

const ENERGY_THRESHOLD: u32 = 10_000;

pub struct ExportColonyView<'a> {
    view: ColonyView<'a>,
    buffer: ColonyBuffer
}

impl<'a> Deref for ExportColonyView<'a> {
    type Target = ColonyView<'a>;

    fn deref(&self) -> &Self::Target {
        &self.view
    }
}

#[derive(Deref)]
pub struct ExportColonies<'a>(HashMap<RoomName, ExportColonyView<'a>>);

impl Colonies {
    pub fn view_export_colonies(&self) -> ExportColonies<'_> {
        ExportColonies(
            self.view_all()
                .filter(|colony| colony.step >= START_EXPORT_STEP)
                .filter_map(|colony| {
                    Some(ExportColonyView {
                        buffer: *colony.buffer.as_ref()?,
                        view: colony
                    })
                })
                .filter(|colony| colony.buffer.used_energy_capacity() > ENERGY_THRESHOLD)
                .map(|colony| (colony.name, colony))
                .collect()
        )
    }
}

impl ImportTruckCreep {
    pub fn roundtrip_ticks(&self) -> Option<f32> {
        if self.trips == 0 { return None }

        Some((2 * self.total_trip_ticks) as f32 / self.trips as f32)
    }

    pub fn update<'a>(&mut self, creep: &mut VirtualCreep, home: &ColonyView<'a>, colonies: &HashMap<RoomName, ExportColonyView<'a>>, coordinator: &mut TruckCoordinator) {
        step(self, |state| state.transition(creep, home, colonies, coordinator));
    }

    fn transition<'a>(&mut self, creep: &mut VirtualCreep, home: &ColonyView<'a>, colonies: &HashMap<RoomName, ExportColonyView<'a>>, coordinator: &mut TruckCoordinator) -> Step<Self> {
        let new_state = self.state.transition(creep, home, colonies, coordinator)?;
        
        if let State::GoingHome(ticks) = self.state && matches!(new_state, State::Arrived(_)) {
            self.trips += 1;
            self.total_trip_ticks += ticks;
        }

        Step::Next(Self { state: new_state, ..*self })
    }
}

impl State {
    fn transition<'a>(&mut self, creep: &mut VirtualCreep, home: &ColonyView<'a>, colonies: &HashMap<RoomName, ExportColonyView<'a>>, coordinator: &mut TruckCoordinator) -> Step<Self> {
        match self {
            Self::Idle => {
                let export_colony = colonies.values()
                    .filter(|colony| home.name != colony.name)
                    .max_by_key(|colony| colony.buffer.used_energy_capacity());
                let Some(export_colony) = export_colony else { return Step::Stay };

                Step::Next(Self::CollectingFrom(export_colony.name))
            },
            Self::CollectingFrom(room_name) => {
                let Some(colony) = colonies.get(room_name) else { next!(Self::Idle) };

                creep.approach(colony.buffer.pos(), 1)?;
                creep.withdraw(colony.buffer, ResourceType::Energy, None)?;
                
                Step::Next(Self::GoingHome(0))
            },
            Self::GoingHome(ticks) => {
                *ticks += 1;
                creep.approach(home.center, 3)?;

                Step::Next(Self::Arrived(ArrivedState::Idle))
            },
            Self::Arrived(arrived_state) => {
                if creep.next_used_energy_capacity() == 0 { 
                    if let ArrivedState::ProvidingTo(consumer) = arrived_state
                        && let Some(handle) = coordinator.consumers.heartbeat(consumer, creep.handle()) {
                            handle.release();
                    }

                    next!(Self::Idle) 
                }

                arrived_state.transition(creep, home, coordinator).map(Self::Arrived)
            }
        }
    }
}

impl ArrivedState {
    fn transition(&mut self, creep: &mut VirtualCreep, home: &ColonyView<'_>, coordinator: &mut TruckCoordinator) -> Step<Self> {
        match self {
            Self::Idle => {
                if let Some(buffer) = home.buffer.as_ref() && buffer.free_energy_capacity() > 0 {
                    next!(Self::StoringAway(*buffer))
                }

                if let Some(consumer) = coordinator.assign_consumer(creep) {
                    next!(Self::ProvidingTo(consumer))
                }

                creep.approach(home.center, 3)?;

                Step::Stay
            },
            Self::ProvidingTo(consumer) => {
                let Some(mut task_handle) = coordinator.consumers.heartbeat(consumer, creep.handle()) else { next!(Self::Idle) };

                creep.approach(consumer.pos(), 1)?;
                task_handle.consume(creep.transfer(consumer.clone(), ResourceType::Energy, None)?);

                task_handle.release();
                Step::Next(Self::Idle)
            },
            Self::StoringAway(buffer) => {
                next_if!(buffer.free_energy_capacity() == 0, Self::Idle);

                creep.approach(buffer.pos(), 1)?;
                creep.transfer(*buffer, ResourceType::Energy, None)?;

                Step::Next(Self::Idle)
            },
        }
    }
}