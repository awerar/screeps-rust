use derive_where::derive_where;
use enum_display::EnumDisplay;
use screeps::{HasPosition, Position, ResourceType};
use serde::Deserialize;
use anyhow::Result;

use crate::{break_deferable, break_move, check::{Check, CheckFrom}, colony::ColonyView, coordination::collaboration::CollaborativeWorkerHandle, creeps::{truck::{TruckCreep::FillingUpFor, coordinator::TruckCoordinator, stop::{ConsumerTruckStop, ProviderTruckStop}}, virtual_creep::{IntentError, VirtualCreep}}, domain_traits::EnergyStoreAccessors, ids::{CheckState, Checked, Unchecked}, movement::requests::MovementRequests, statemachine::Transition};

#[derive(Debug, Default, EnumDisplay)]
#[derive_where(Serialize, Deserialize, Clone; TruckTask<S>, ConsumerTruckStop<S>)]
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
    fn finish_task(task_handle: CollaborativeWorkerHandle<'_>) -> Self {
        task_handle.remove();
        Self::Idle
    }

    pub fn update(self, truck: &mut VirtualCreep, home: &ColonyView<'_>, movement: &mut MovementRequests, coordinator: &mut TruckCoordinator) -> Result<Transition<Self>> {
        use Transition::*;

        match self {
            Self::Idle => {
                if truck.next_used_energy_capacity() > 0 {
                    let consumer = coordinator.assign_consumer(truck);
                    if let Some(consumer) = consumer { return Ok(Continue(Self::Performing(TruckTask::ProvidingTo(consumer)))) }

                    if home.buffer.as_ref().is_some_and(|buffer| buffer.free_energy_capacity() > 0) { 
                        return Ok(Continue(Self::StoringAway)) 
                    }
                } else {
                    let push_provider = coordinator.assign_push_provider(truck);
                    if let Some(provider) = push_provider { return Ok(Continue(Self::Performing(TruckTask::CollectingFrom(provider)))) }

                    if home.buffer.as_ref().is_some_and(|buffer| buffer.used_energy_capacity() > 0) {
                        let consumer = coordinator.assign_consumer(truck);
                        if let Some(consumer) = consumer { return Ok(Continue(Self::FillingUpFor(consumer))) }
                    }

                    let provider = coordinator.assign_provider(truck);
                    if let Some(provider) = provider { return Ok(Continue(Self::Performing(TruckTask::CollectingFrom(provider)))) }
                }

                Ok(Break(self))
            },
            Self::Performing(ref task) => {
                let Some(mut handle) = coordinator.heartbeat(truck, task) else { return Ok(Continue(Self::Idle)) };

                match task {
                    TruckTask::CollectingFrom(_) => 
                        if truck.next_free_capacity() == 0 { return Ok(Continue(Self::finish_task(handle))) },
                    TruckTask::ProvidingTo(task) => 
                        if truck.next_used_energy_capacity() == 0 { return Ok(Continue(FillingUpFor(task.clone()))) },
                }

                break_deferable!(break_move!(movement.move_vcreep_to(truck, task.pos(), 1), self), self)?;

                if truck.incoming_energy() > 0 { return Ok(Break(self)) }
                handle.apply_work(break_deferable!(task.creep_perform(truck), self)?);

                Ok(Continue(Self::finish_task(handle)))
            },
            Self::FillingUpFor(ref consumer) => {
                let Some(mut handle) = coordinator.consumers.heartbeat(consumer, truck.handle()) else { return Ok(Continue(Self::Idle)) };

                let Some(buffer) = home.buffer.as_ref().filter(|buffer| buffer.used_energy_capacity() > 0) else {
                    return Ok(Continue(Self::finish_task(handle)))
                };

                if truck.next_used_energy_capacity() > 0 { 
                    return Ok(Continue(Self::Performing(TruckTask::ProvidingTo(consumer.clone())))) 
                }

                break_deferable!(break_move!(movement.move_vcreep_to(truck, buffer.pos(), 1), self), self)?;

                if truck.outgoing() > 0 { return Ok(Break(self)) }
                handle.apply_work(break_deferable!(truck.withdraw(buffer.clone(), ResourceType::Energy, None), self)?);

                Ok(Continue(Self::Performing(TruckTask::ProvidingTo(consumer.clone()))))
            },
            Self::StoringAway => {
                let Some(buffer) = home.buffer.as_ref().filter(|buffer| buffer.free_energy_capacity() > 0) else { 
                    return Ok(Continue(Self::Idle)) 
                };
                
                if truck.next_used_energy_capacity() == 0 { return Ok(Continue(Self::Idle)); }

                break_deferable!(break_move!(movement.move_vcreep_to(truck, buffer.pos(), 1), self), self)?;
                
                if truck.incoming_energy() > 0 { return Ok(Break(self)) }
                break_deferable!(truck.transfer(buffer.clone(), ResourceType::Energy, None), self)?;

                Ok(Continue(Self::Idle))
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

    fn creep_perform(&self, truck: &mut VirtualCreep) -> anyhow::Result<u32, IntentError> {
        match self {
            TruckTask::CollectingFrom(provider) => 
                provider.creep_withdraw(truck, ResourceType::Energy),
            TruckTask::ProvidingTo(consumer) => 
                truck.transfer(consumer.clone(), ResourceType::Energy, None)
        }
    }
}