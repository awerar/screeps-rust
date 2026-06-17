use enum_display::EnumDisplay;
use screeps::{Creep, HasPosition, Position, ResourceType};
use serde::{Deserialize, Serialize};
use anyhow::Result;

use crate::{break_collision, break_move, colony::ColonyView, creeps::{truck::{TruckCreep::FillingUpFor, coordinator::TruckCoordinator, stop::{ConsumerTruckStop, ProviderTruckStop}}, virtual_creep::{IntentError, StoreTarget, VirtualCreep}}, movement::requests::MovementRequests, safeid::{DO, DumbID, IDKind, SafeID, SafeIDs, TryFromUnsafe, TryMakeSafe, UnsafeIDs}, statemachine::{Transition, update_many}, utils::EnergyStore};

#[derive(Serialize, Deserialize, Debug, Clone, Default, EnumDisplay)]
#[serde(bound(deserialize = "TruckTask<I> : DO, ConsumerTruckStop<I> : DO"))]
pub enum TruckCreep<I: IDKind = SafeIDs> {
    #[default] Idle,
    Performing(TruckTask<I>),
    StoringAway,
    FillingUpFor(ConsumerTruckStop<I>)
}

impl<'de> Deserialize<'de> for TruckCreep {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let us = TruckCreep::<UnsafeIDs>::deserialize(deserializer)?;
        Ok(match us {
            TruckCreep::Idle => Self::Idle,
            TruckCreep::Performing(x) => 
                x.try_make_safe().map_or(Self::Idle, Self::Performing),
            TruckCreep::StoringAway => Self::StoringAway,
            TruckCreep::FillingUpFor(x) => 
                x.try_make_safe().map_or(Self::Idle, Self::FillingUpFor),
        })
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(bound(deserialize = "ProviderTruckStop<I> : DO, ConsumerTruckStop<I> : DO"))]
pub enum TruckTask<I: IDKind = SafeIDs> {
    CollectingFrom(ProviderTruckStop<I>),
    ProvidingTo(ConsumerTruckStop<I>)
}

impl TryFromUnsafe for TruckTask {
    type Unsafe = TruckTask<UnsafeIDs>;

    fn try_from_unsafe(us: Self::Unsafe) -> Option<Self> {
        Some(match us {
            Self::Unsafe::CollectingFrom(x) => Self::CollectingFrom(x.try_make_safe()?),
            Self::Unsafe::ProvidingTo(x) => Self::ProvidingTo(x.try_make_safe()?),
        })
    }
}

impl TruckCreep {
    fn task(&self) -> Option<TruckTask> {
        match self {
            Self::Performing(task) => Some(task.clone()),
            Self::FillingUpFor(consumer) => Some(consumer.clone().into()),
            _ => None
        }
    }

    pub fn update(mut self, creep: &SafeID<Creep>, home: &ColonyView<'_>, movement: &mut MovementRequests, coordinator: &mut TruckCoordinator) -> Self {
        self = self.validate_task(&creep.dumb_id(), coordinator);

        let mut virtual_creep = VirtualCreep::new(creep.clone());
        update_many(self, |state| state.execute_and_finish_task_on_err(&mut virtual_creep, home, movement, coordinator))
    }

    fn validate_task(self, creep: &DumbID<Creep>, coordinator: &mut TruckCoordinator) -> Self {
        let Some(task) = self.task() else { return self; };

        if !coordinator.heartbeat(creep, &task) { return Self::Idle }
        if !task.still_valid() { return Self::fail(creep, &task, coordinator) }

        self
    }

    fn fail(creep: &DumbID<Creep>, task: &TruckTask, coordinator: &mut TruckCoordinator) -> Self {
        coordinator.finish(creep, task, false);
        Self::Idle
    }

    fn succeed(creep: &DumbID<Creep>, task: &TruckTask, coordinator: &mut TruckCoordinator) -> Self {
        coordinator.finish(creep, task, true);
        Self::Idle
    }

    pub fn execute_and_finish_task_on_err(self, truck: &mut VirtualCreep, home: &ColonyView<'_>, movement: &mut MovementRequests, coordinator: &mut TruckCoordinator) -> Result<Transition<Self>> {
        let task = self.task();

        let result = self.execute(truck, home, movement, coordinator);
        if let Some(task) = task && result.is_err() {
            coordinator.finish(&truck.id(), &task, false);
        }

        result
    }

    fn execute(self, truck: &mut VirtualCreep, home: &ColonyView<'_>, movement: &mut MovementRequests, coordinator: &mut TruckCoordinator) -> Result<Transition<Self>> {
        use Transition::*;

        match self {
            Self::Idle => {
                if truck.next_used_energy_capacity() > 0 {
                    let consumer = coordinator.assign_consumer(truck);
                    if let Some(consumer) = consumer { return Ok(Continue(Self::Performing(TruckTask::ProvidingTo(consumer)))) }

                    if home.buffer.as_ref().is_some_and(|buffer| buffer.store().free_energy_capacity() > 0) { 
                        return Ok(Continue(Self::StoringAway)) 
                    }
                } else {
                    let push_provider = coordinator.assign_push_provider(truck);
                    if let Some(provider) = push_provider { return Ok(Continue(Self::Performing(TruckTask::CollectingFrom(provider)))) }

                    if home.buffer.as_ref().is_some_and(|buffer| buffer.store().used_energy_capacity() > 0) {
                        let consumer = coordinator.assign_consumer(truck);
                        if let Some(consumer) = consumer { return Ok(Continue(Self::FillingUpFor(consumer))) }
                    }

                    let provider = coordinator.assign_provider(truck);
                    if let Some(provider) = provider { return Ok(Continue(Self::Performing(TruckTask::CollectingFrom(provider)))) }
                }

                Ok(Break(self))
            },
            Self::Performing(ref task) => {
                match task {
                    TruckTask::CollectingFrom(_) => 
                        if truck.next_free_capacity() == 0 { return Ok(Continue(Self::fail(&truck.id(), task, coordinator))) },
                    TruckTask::ProvidingTo(task) => 
                        if truck.next_used_energy_capacity() == 0 { return Ok(Continue(FillingUpFor(task.clone()))) },
                }

                break_collision!(break_move!(movement.move_vcreep_to(truck, task.pos(), 1), self), self)?;

                if truck.incoming_energy() > 0 { return Ok(Break(self)) }
                break_collision!(task.creep_perform(truck), self)?;

                Ok(Continue(Self::succeed(&truck.id(), task, coordinator)))
            },
            Self::FillingUpFor(ref consumer) => {
                let Some(buffer) = home.buffer.as_ref().filter(|buffer| buffer.store().used_energy_capacity() > 0) else {
                    return Ok(Continue(Self::fail(&truck.id(), &consumer.clone().into(), coordinator)))
                };

                if truck.next_used_energy_capacity() > 0 { 
                    return Ok(Continue(Self::Performing(TruckTask::ProvidingTo(consumer.clone())))) 
                }

                break_collision!(break_move!(movement.move_vcreep_to(truck, buffer.pos(), 1), self), self)?;

                if truck.outgoing() > 0 { return Ok(Break(self)) }
                break_collision!(truck.withdraw(buffer, ResourceType::Energy, None), self)?;

                Ok(Continue(Self::Performing(TruckTask::ProvidingTo(consumer.clone()))))
            },
            Self::StoringAway => {
                let Some(buffer) = home.buffer.as_ref().filter(|buffer| buffer.store().free_energy_capacity() > 0) else { 
                    return Ok(Continue(Self::Idle)) 
                };
                
                if truck.next_used_energy_capacity() == 0 { return Ok(Continue(Self::Idle)); }

                break_collision!(break_move!(movement.move_vcreep_to(truck, buffer.pos(), 1), self), self)?;
                
                if truck.incoming_energy() > 0 { return Ok(Break(self)) }
                break_collision!(truck.transfer(buffer, ResourceType::Energy, None), self)?;

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

    fn creep_perform(&self, truck: &mut VirtualCreep) -> anyhow::Result<(), IntentError> {
        match self {
            TruckTask::CollectingFrom(provider) => 
                provider.creep_withdraw(truck, ResourceType::Energy),
            TruckTask::ProvidingTo(consumer) => 
                consumer.creep_transfer(truck, ResourceType::Energy)
        }
    }

    fn still_valid(&self) -> bool {
        match self {
            TruckTask::CollectingFrom(provider) => 
                provider.get_resource_avaliable(ResourceType::Energy) > 0,
            TruckTask::ProvidingTo(consumer) =>
                consumer.get_resource_free(ResourceType::Energy) > 0
        }
    }
}