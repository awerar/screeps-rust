use std::{collections::HashMap, fmt::Display, mem, ops::Deref};

use derive_deref::Deref;
use derive_where::derive_where;
use enum_display::EnumDisplay;
use screeps::{HasPosition, ResourceType, RoomName};
use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::{check::Check, colony::{Colonies, ColonyBuffer, ColonyView, steps::ColonyStep}, creeps::{truck::{TruckCoordinator, stop::ConsumerTruckStop}, virtual_creep::VirtualCreep}, domain_traits::EnergyStoreAccessors, goto, stay, ids::{CheckState, Checked, Unchecked}, statemachine::{Flow, Next, step}};

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

    pub fn update<'a>(&mut self, creep: &mut VirtualCreep, home: &ColonyView<'a>, colonies: &HashMap<RoomName, ExportColonyView<'a>>, coordinator: &mut TruckCoordinator) -> Result<()> {
        *self = step(mem::take(self), |state| state.transition(creep, home, colonies, coordinator))?;
        Ok(())
    }

    fn transition<'a>(mut self, creep: &mut VirtualCreep, home: &ColonyView<'a>, colonies: &HashMap<RoomName, ExportColonyView<'a>>, coordinator: &mut TruckCoordinator) -> Flow<Next<Self>> {
        let going_home_ticks = if let State::GoingHome(ticks) = self.state { Some(ticks) } else { None };
        let next = self.state.transition(creep, home, colonies, coordinator)?;

        Flow::Continue(next.map(|state| {
            if let Some(ticks) = going_home_ticks && matches!(state, State::Arrived(_)) {
                self.trips += 1;
                self.total_trip_ticks += ticks;
            }

            Self { state, ..self }
        }))
    }
}

impl State {
    fn transition<'a>(self, creep: &mut VirtualCreep, home: &ColonyView<'a>, colonies: &HashMap<RoomName, ExportColonyView<'a>>, coordinator: &mut TruckCoordinator) -> Flow<Next<Self>> {
        match self {
            Self::Idle => {
                let export_colony = colonies.values()
                    .filter(|colony| home.name != colony.name)
                    .max_by_key(|colony| colony.buffer.used_energy_capacity());
                let Some(export_colony) = export_colony else { return Flow::Break };

                goto!(Self::CollectingFrom(export_colony.name))
            },
            Self::CollectingFrom(room_name) => {
                let Some(colony) = colonies.get(&room_name) else { goto!(Self::Idle) };

                creep.approach(colony.buffer.pos(), 1)?;
                creep.withdraw(colony.buffer, ResourceType::Energy, None)?;
                
                goto!(Self::GoingHome(1))
            },
            Self::GoingHome(ticks) => match creep.approach(home.center, 3) {
                Flow::Continue(()) => goto!(Self::Arrived(ArrivedState::Idle)),
                Flow::Break => stay!(Self::GoingHome(ticks + 1)),
                Flow::Fail(e) => Flow::Fail(e),
            },
            Self::Arrived(arrived_state) => {
                if creep.next_used_energy_capacity() == 0 { 
                    if let ArrivedState::ProvidingTo(consumer) = &arrived_state
                        && let Some(handle) = coordinator.consumers.heartbeat(consumer, creep.handle()) {
                            handle.release();
                    }

                    goto!(Self::Idle) 
                }

                arrived_state.transition(creep, home, coordinator).map_continue(|next| next.map(Self::Arrived))
            }
        }
    }
}

impl ArrivedState {
    fn transition(self, creep: &mut VirtualCreep, home: &ColonyView<'_>, coordinator: &mut TruckCoordinator) -> Flow<Next<Self>> {
        match self {
            Self::Idle => {
                goto!(if let Some(buffer) = home.buffer.as_ref().filter(|b| b.free_energy_capacity() > 0) { Self::StoringAway(*buffer) });
                goto!(if let Some(consumer) = coordinator.assign_consumer(creep) { Self::ProvidingTo(consumer) });

                creep.approach(home.center, 3)?;

                Flow::Break
            },
            Self::ProvidingTo(consumer) => {
                let Some(mut task_handle) = coordinator.consumers.heartbeat(&consumer, creep.handle()) else { goto!(Self::Idle) };

                creep.approach(consumer.pos(), 1)?;
                task_handle.consume(creep.transfer(consumer, ResourceType::Energy, None)?);

                task_handle.release();
                goto!(Self::Idle)
            },
            Self::StoringAway(buffer) => {
                goto!(if buffer.free_energy_capacity() == 0 { Self::Idle });

                creep.approach(buffer.pos(), 1)?;
                creep.transfer(buffer, ResourceType::Energy, None)?;

                goto!(Self::Idle)
            },
        }
    }
}