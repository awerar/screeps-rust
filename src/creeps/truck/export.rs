use std::{fmt::Display, mem};

use derive_where::derive_where;
use enum_display::EnumDisplay;
use screeps::{CARRY_CAPACITY, HasPosition, Part, ResourceType, RoomName, pathfinder::{self, SearchOptions}};
use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::{check::Check, colony::{ColonyBuffer, ColonyView}, creeps::{body::Body, truck::{TruckCoordinator, stop::ConsumerTruckStop}, virtual_creep::VirtualCreep}, domain_traits::EnergyStoreAccessors, goto, ids::{CheckState, Checked, Unchecked}, statemachine::{Flow, OrBreak, step}};

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
    pub fn new(home: &ColonyView<'_>, import_colony: &ColonyView<'_>) -> Self {
        let export2import_ticks = pathfinder::search(home.center, import_colony.center, 0, Some(SearchOptions::default())).cost();
        let import2export_ticks = pathfinder::search(import_colony.center, home.center, 0, Some(SearchOptions::default().swamp_cost(1))).cost();

        Self { 
            state: State::default(), 
            import_colony: import_colony.name, 
            roundtrip_ticks: export2import_ticks + import2export_ticks
        }
    }
    
    pub fn roundtrip_ticks(&self) -> u32 {
        self.roundtrip_ticks.max(1)
    }

    pub fn power(&self, body: &Body) -> f32 {
        (body.part_count(Part::Carry) as f32 * CARRY_CAPACITY as f32) / self.roundtrip_ticks() as f32
    }

    pub fn import_colony(&self) -> RoomName {
        self.import_colony
    }

    pub fn update(&mut self, creep: &mut VirtualCreep, home: &ColonyView<'_>, import_colony: &ColonyView<'_>, import_coordinator: &mut TruckCoordinator) -> Result<()> {
        self.state = step(mem::take(&mut self.state), |state| state.transition(creep, home, import_colony, import_coordinator))?;
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

                creep.approach(import_colony.center, 5)?;

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