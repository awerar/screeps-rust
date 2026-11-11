use derive_where::derive_where;
use enum_display::EnumDisplay;
use screeps::{HasPosition, Position, ResourceType};
use serde::Deserialize;
use anyhow::Result;

use crate::{check::{Check, CheckFrom}, colony::ColonyView, coordination::allocations::CreepAllocationHandle, creeps::{truck::{TruckCreep::FillingUpFor, coordinator::TruckCoordinator, stop::{ConsumerTruckStop, ProviderTruckStop}}, virtual_creep::VirtualCreep}, domain_traits::EnergyStoreAccessors, ids::{CheckState, Checked, Unchecked}, goto, statemachine::{Flow, step, break_if}};

#[derive(Debug, Default, EnumDisplay)]
#[derive_where(Serialize, Deserialize, Clone; TruckTask<S>, ConsumerTruckStop<S>, S)]
pub enum TruckCreep<S: CheckState = Checked> {
    #[default] Idle,
    Performing(TruckTask<S>),
    StoringAway,
    FillingUpFor(ConsumerTruckStop<S>)
}

impl<'de> Deserialize<'de> for TruckCreep {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let us = TruckCreep::<Unchecked>::deserialize(deserializer)?;
        Ok(match us {
            TruckCreep::Idle => Self::Idle,
            TruckCreep::Performing(x) => 
                x.check().map_or(Self::Idle, Self::Performing),
            TruckCreep::StoringAway => Self::StoringAway,
            TruckCreep::FillingUpFor(x) => 
                x.check().map_or(Self::Idle, Self::FillingUpFor),
        })
    }
}

#[derive(Debug)]
#[derive_where(Serialize, Deserialize, Clone; ProviderTruckStop<S>, ConsumerTruckStop<S>)]
pub enum TruckTask<S: CheckState = Checked> {
    CollectingFrom(ProviderTruckStop<S>),
    ProvidingTo(ConsumerTruckStop<S>)
}

impl CheckFrom for TruckTask {
    type Unchecked = TruckTask<Unchecked>;
    type Err = anyhow::Error;

    fn check_from(us: Self::Unchecked) -> Result<Self, Self::Err> {
        Ok(match us {
            Self::Unchecked::CollectingFrom(x) => Self::CollectingFrom(x.check()?),
            Self::Unchecked::ProvidingTo(x) => Self::ProvidingTo(x.check()?),
        })
    }
}

impl TruckCreep {
    fn finish_task(task_handle: CreepAllocationHandle<'_>) -> Self {
        task_handle.release();
        Self::Idle
    }

    pub fn update(&mut self, truck: &mut VirtualCreep, home: &ColonyView<'_>, coordinator: &mut TruckCoordinator) {
        step(self, |state| state.transition(truck, home, coordinator));
    }

    fn transition(&mut self, truck: &mut VirtualCreep, home: &ColonyView<'_>, coordinator: &mut TruckCoordinator) -> Flow<Self> {
        match self {
            Self::Idle => {
                if truck.next_used_energy_capacity() > 0 {
                    goto!(if let Some(consumer) = coordinator.assign_consumer(truck) { Self::Performing(TruckTask::ProvidingTo(consumer)) });
                    goto!(if home.buffer.as_ref().is_some_and(|buffer| buffer.free_energy_capacity() > 0) { Self::StoringAway });
                } else {
                    goto!(if let Some(provider) = coordinator.assign_push_provider(truck) { Self::Performing(TruckTask::CollectingFrom(provider)) });

                    goto!(if home.buffer.as_ref().is_some_and(|buffer| buffer.used_energy_capacity() > 0)
                        && let Some(consumer) = coordinator.assign_consumer(truck) {
                            Self::FillingUpFor(consumer)
                    });

                    goto!(if let Some(provider) = coordinator.assign_provider(truck) { Self::Performing(TruckTask::CollectingFrom(provider)) });
                }

                if let Some(buffer) = home.buffer.as_ref() {
                    truck.approach(buffer.pos(), 2)?;
                }

                Flow::Break
            },
            Self::Performing(task) => {
                let Some(mut handle) = coordinator.heartbeat(truck, task) else { goto!(Self::Idle) };

                match task {
                    TruckTask::CollectingFrom(_) => 
                        goto!(if truck.next_free_capacity() == 0 { Self::finish_task(handle) }),
                    TruckTask::ProvidingTo(task) => 
                        goto!(if truck.next_used_energy_capacity() == 0 { FillingUpFor(task.clone()) })
                }

                truck.approach(task.pos(), 1)?;

                break_if(truck.incoming_energy() > 0)?;
                handle.consume(task.creep_perform(truck)?);

                Flow::Continue(Self::finish_task(handle))
            },
            Self::FillingUpFor(consumer) => {
                let Some(mut handle) = coordinator.consumers.heartbeat(consumer, truck.handle()) else { goto!(Self::Idle) };

                let Some(buffer) = home.buffer.as_ref().filter(|buffer| buffer.used_energy_capacity() > 0) else {
                    goto!(Self::finish_task(handle))
                };

                goto!(if truck.next_used_energy_capacity() > 0 { Self::Performing(TruckTask::ProvidingTo(consumer.clone())) });

                truck.approach(buffer.pos(), 1)?;

                break_if(truck.outgoing() > 0)?;
                handle.consume(truck.withdraw(*buffer, ResourceType::Energy, None)?);

                Flow::Continue(Self::Performing(TruckTask::ProvidingTo(consumer.clone())))
            },
            Self::StoringAway => {
                let Some(buffer) = home.buffer.as_ref().filter(|buffer| buffer.free_energy_capacity() > 0) else { 
                    goto!(Self::Idle)
                };
                
                goto!(if truck.next_used_energy_capacity() == 0 { Self::Idle });

                truck.approach(buffer.pos(), 1)?;
                
                break_if(truck.incoming_energy() > 0)?;
                truck.transfer(*buffer, ResourceType::Energy, None)?;

                Flow::Continue(Self::Idle)
            },
        }
    }
}

impl From<ConsumerTruckStop> for TruckTask {
    fn from(value: ConsumerTruckStop) -> Self {
        TruckTask::ProvidingTo(value)
    }
}

impl From<ProviderTruckStop> for TruckTask {
    fn from(value: ProviderTruckStop) -> Self {
        TruckTask::CollectingFrom(value)
    }
}

impl TruckTask {
    fn pos(&self) -> Position {
        match self {
            TruckTask::CollectingFrom(provider) => provider.pos(),
            TruckTask::ProvidingTo(consumer) => consumer.pos()
        }
    }

    fn creep_perform(&self, truck: &mut VirtualCreep) -> Flow<u32> {
        match self {
            TruckTask::CollectingFrom(provider) => 
                provider.creep_withdraw(truck, ResourceType::Energy),
            TruckTask::ProvidingTo(consumer) => 
                truck.transfer(consumer.clone(), ResourceType::Energy, None)
        }
    }
}