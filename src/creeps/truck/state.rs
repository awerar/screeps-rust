use enum_display::EnumDisplay;
use screeps::{Creep, HasPosition, MaybeHasId, Position, ResourceType, SharedCreepProperties};
use serde::{Deserialize, Serialize};

use crate::{colony::ColonyView, creeps::truck::{coordinator::TruckCoordinator, stop::{ConsumerTruckStop, ProviderTruckStop}}, movement::requests::MovementRequests, safeid::{DO, IDKind, SafeID, SafeIDs, TryFromUnsafe, TryMakeSafe, UnsafeIDs}, statemachine::Transition, utils::EnergyStore};

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
    pub fn update(self, creep: &SafeID<Creep>, home: &ColonyView<'_>, movement: &mut MovementRequests, coordinator: &mut TruckCoordinator, already_transfered: &mut bool, delta: &mut i32) -> anyhow::Result<Transition<Self>> {
        use Transition::*;

        let fail_task_transition = |task, coordinator: &mut TruckCoordinator| {
            coordinator.finish(creep, task, false);
            Ok(Transition::Continue(Self::Idle))
        };

        let creep_id = creep.try_id().unwrap();
        let fail_consumer_task_transition = |task, coordinator: &mut TruckCoordinator| {
            coordinator.consumers.finish_task(creep_id, task, false);
            anyhow::Ok(Transition::Continue(Self::Idle))
        };

        match self {
            Self::Idle => {
                if creep.store().used_energy_capacity().strict_add_signed(*delta) >  0 {
                    let consumer = coordinator.assign_consumer(creep, *delta);
                    if let Some(consumer) = consumer { return Ok(Continue(Self::Performing(TruckTask::ProvidingTo(consumer)))) }

                    if home.buffer.as_ref().is_some_and(|buffer| buffer.energy_capacity_left() > 0) { 
                        return Ok(Continue(Self::StoringAway)) 
                    }
                } else {
                    let push_provider = coordinator.assign_push_provider(creep, *delta);
                    if let Some(provider) = push_provider { return Ok(Continue(Self::Performing(TruckTask::CollectingFrom(provider)))) }

                    if home.buffer.as_ref().is_some_and(|buffer| buffer.energy() > 0) {
                        let consumer = coordinator.assign_consumer(creep, *delta);
                        if let Some(consumer) = consumer { return Ok(Continue(Self::FillingUpFor(consumer))) }
                    }

                    let provider = coordinator.assign_provider(creep, *delta);
                    if let Some(provider) = provider { return Ok(Continue(Self::Performing(TruckTask::CollectingFrom(provider)))) }
                }

                Ok(Break(self))
            },
            Self::Performing(ref task) => {
                if !task.still_valid() || !coordinator.heartbeat(creep, task) { return fail_task_transition(task, coordinator) }
                
                if movement.move_creep_to(creep, task.pos(), 1).in_range() && !*already_transfered {
                    *delta = task.creep_perform(creep)?;
                    coordinator.finish(creep, task, true);
                    *already_transfered = true;
                    return Ok(Continue(Self::Idle))
                }
                    
                Ok(Break(self))
            },
            Self::FillingUpFor(ref consumer) => {
                let Some(buffer) = &home.buffer else { return fail_consumer_task_transition(consumer, coordinator) };
                if buffer.energy() == 0 || !coordinator.consumers.heartbeat_task(creep, consumer) { return fail_consumer_task_transition(consumer, coordinator) }

                if movement.move_creep_to(creep, buffer.pos(), 1).in_range() {
                    creep.withdraw(buffer.withdrawable(), ResourceType::Energy, None).ok();
                    return Ok(Break(Self::Performing(TruckTask::ProvidingTo(consumer.clone()))))
                }
                    
                Ok(Break(self))
            },
            Self::StoringAway => {
                let Some(buffer) = &home.buffer else { return Ok(Continue(Self::Idle)) };
                if buffer.energy_capacity_left() == 0 { return Ok(Continue(Self::Idle)) }
                
                if movement.move_creep_to(creep, buffer.pos(), 1).in_range() {
                    creep.transfer(buffer.transferable(), ResourceType::Energy, None).ok();
                    return Ok(Break(Self::Idle))
                }

                Ok(Break(self))
            },
        }
    }
}

impl TruckTask {
    fn pos(&self) -> Position {
        match self {
            TruckTask::CollectingFrom(provider) => provider.pos(),
            TruckTask::ProvidingTo(consumer) => consumer.pos()
        }
    }

    fn creep_perform(&self, creep: &Creep) -> anyhow::Result<i32> {
        match self {
            TruckTask::CollectingFrom(provider) => {
                provider.creep_withdraw(creep, ResourceType::Energy)?;

                let creep_avaliable = creep.store().free_energy_capacity();
                let provider_avaliable: i32 = provider.get_resource_avaliable(ResourceType::Energy).try_into().unwrap();
                Ok(provider_avaliable.min(creep_avaliable))
            },
            TruckTask::ProvidingTo(consumer) => {
                consumer.creep_transfer(creep, ResourceType::Energy)?;

                let creep_avaliable = creep.store().used_energy_capacity();
                let consumer_avaliable = consumer.get_resource_free(ResourceType::Energy);
                Ok(-i32::try_from(consumer_avaliable.min(creep_avaliable)).unwrap())
            }
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