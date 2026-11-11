use std::{fmt::Display, mem};

use derive_where::derive_where;
use enum_display::EnumDisplay;
use screeps::{CARRY_CAPACITY, HasPosition, Part, ResourceType, RoomName};
use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::{check::Check, colony::{Colonies, ColonyBuffer, ColonyView, steps::ColonyStep}, creeps::{truck::{TruckCoordinator, stop::ConsumerTruckStop}, virtual_creep::VirtualCreep}, domain_traits::EnergyStoreAccessors, goto, ids::{CheckState, Checked, Unchecked}, spawn::prototype::Body, statemachine::{Flow, OrBreak, step}};

pub const STOP_IMPORT_STEP: ColonyStep = ColonyStep::UpgradeToLevel5;
const START_EXPORT_STEP: ColonyStep = ColonyStep::UpgradeToLevel6;

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ExportTruckCreep {
    state: State,
    import_colony: RoomName,
    roundtrip_ticks: u32
}

impl Display for ExportTruckCreep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} [{:?} rtt]", self.state, self.roundtrip_ticks())
    }
}

#[derive(Deserialize, Serialize, Clone, Debug, Default, EnumDisplay)]
pub enum State {
    #[default] GoingHome,
    GoingAway,
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

impl ExportTruckCreep {
    pub fn roundtrip_ticks(&self) -> u32 {
        self.roundtrip_ticks.max(1)
    }

    pub fn power(&self, body: &Body) -> f32 {
        (body.part_count(Part::Carry) as f32 * CARRY_CAPACITY as f32) / self.roundtrip_ticks() as f32
    }

    pub fn import_colony(&self) -> RoomName {
        self.import_colony
    }

    pub fn update<'a>(&mut self, creep: &mut VirtualCreep, home: &ColonyView<'a>, colonies: &Colonies, coordinator: &mut TruckCoordinator) -> Result<()> {
        // TODO: Instead scrap
        let import_colony = colonies.view(self.import_colony).ok_or(anyhow!("Import colony doesn't exist"))?;
        self.state = step(mem::take(&mut self.state), |state| state.transition(creep, home, &import_colony, coordinator))?;
        Ok(())
    }
}

impl State {
    fn transition<'a>(self, creep: &mut VirtualCreep, home: &ColonyView<'a>, import_colony: &ColonyView<'a>, coordinator: &mut TruckCoordinator) -> Flow<Self> {
        match self {
            Self::GoingHome => {
                goto!(if creep.next_used_energy_capacity() > 0 { Self::GoingAway });

                let buffer = home.buffer.or_break()?;

                creep.approach(buffer.pos(), 1)?;
                creep.withdraw(buffer, ResourceType::Energy, None)?;
                
                Flow::Continue(Self::GoingAway)
            },
            Self::GoingAway => {
                creep.approach(import_colony.center, 3)?;

                Flow::Continue(Self::Arrived(ArrivedState::Idle))
            },
            Self::Arrived(arrived_state) => {
                if creep.next_used_energy_capacity() == 0 { 
                    arrived_state.release_task(creep, coordinator);

                    goto!(Self::GoingHome) 
                }

                arrived_state.transition(creep, import_colony, coordinator).map_continue(Self::Arrived)
            }
        }
    }
}

impl ArrivedState {
    fn release_task(&self, creep: &VirtualCreep, coordinator: &mut TruckCoordinator) {
        let ArrivedState::ProvidingTo(consumer) = self else { return };
        let Some(handle) = coordinator.consumers.heartbeat(consumer, creep.handle()) else { return }; 
        handle.release();
    }

    fn transition(self, creep: &mut VirtualCreep, import_colony: &ColonyView<'_>, coordinator: &mut TruckCoordinator) -> Flow<Self> {
        match self {
            Self::Idle => {
                goto!(if let Some(buffer) = import_colony.buffer.as_ref() && buffer.free_energy_capacity() > 0 { Self::StoringAway(*buffer) });
                goto!(if let Some(consumer) = coordinator.assign_consumer(creep) { Self::ProvidingTo(consumer) });

                creep.approach(import_colony.center, 3)?;

                Flow::Break
            },
            Self::ProvidingTo(consumer) => {
                let Some(mut task_handle) = coordinator.consumers.heartbeat(&consumer, creep.handle()) else { goto!(Self::Idle) };

                creep.approach(consumer.pos(), 1)?;
                task_handle.consume(creep.transfer(consumer, ResourceType::Energy, None)?);

                task_handle.release();
                Flow::Continue(Self::Idle)
            },
            Self::StoringAway(buffer) => {
                goto!(if buffer.free_energy_capacity() == 0 { Self::Idle });

                creep.approach(buffer.pos(), 1)?;
                creep.transfer(buffer, ResourceType::Energy, None)?;

                Flow::Continue(Self::Idle)
            },
        }
    }
}