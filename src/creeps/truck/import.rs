use std::collections::HashMap;

use derive_where::derive_where;
use enum_display::EnumDisplay;
use screeps::{HasPosition, ResourceType, RoomName};
use serde::Deserialize;
use anyhow::{Result, anyhow};

use crate::{check::Check, colony::{ColonyBuffer, ColonyView, steps::ColonyStep}, coordination::allocations::CreepAllocationHandle, creeps::{truck::{TruckCoordinator, stop::ConsumerTruckStop}, virtual_creep::VirtualCreep}, defer, defer_err, domain_traits::EnergyStoreAccessors, done, ids::{CheckState, Checked, Unchecked}, movement::requests::MovementRequests, next, next_if, statemachine::Transition};

pub const STOP_IMPORT_STEP: ColonyStep = ColonyStep::UpgradeToLevel5;
const START_EXPORT_STEP: ColonyStep = ColonyStep::UpgradeToLevel6;

#[derive(Debug, Default, EnumDisplay)]
#[derive_where(Serialize, Deserialize, Clone; ConsumerTruckStop<S>, ColonyBuffer<S>, S)]
pub enum ImportTruckState<S: CheckState = Checked> {
    #[default] Idle,
    CollectingFrom(RoomName),
    GoingHome,
    ProvidingIdle,
    ProvidingTo(ConsumerTruckStop<S>),
    StoringAway(ColonyBuffer<S>)
}

impl<'de> Deserialize<'de> for ImportTruckState {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let us = ImportTruckState::<Unchecked>::deserialize(deserializer)?;
        Ok(match us {
            ImportTruckState::Idle => Self::Idle,
            ImportTruckState::CollectingFrom(room) => Self::CollectingFrom(room),
            ImportTruckState::GoingHome => Self::GoingHome,
            ImportTruckState::ProvidingIdle => Self::ProvidingIdle,
            ImportTruckState::ProvidingTo(consumer) => 
                consumer.check().map_or(Self::ProvidingIdle, Self::ProvidingTo),
            ImportTruckState::StoringAway(buffer) => 
                buffer.check().map_or(Self::ProvidingIdle, Self::StoringAway)
        })
    }
}

const ENERGY_THRESHOLD: u32 = 10_000;

impl ImportTruckState {
    fn finish_task(task_handle: CreepAllocationHandle<'_>) -> Self {
        task_handle.release();
        Self::ProvidingIdle
    }

    pub fn update<'a>(self, creep: &mut VirtualCreep, home: &ColonyView<'a>, colonies: &HashMap<RoomName, ColonyView<'a>>, movement: &mut MovementRequests, coordinator: &mut TruckCoordinator) -> Result<Transition<Self>> {
        use Transition::*;

        match self {
            Self::Idle => {
                let export_colony = colonies.values()
                    .filter(|colony| colony.step >= START_EXPORT_STEP)
                    .max_by_key(|colony| colony.buffer.as_ref().map_or(0, EnergyStoreAccessors::used_energy_capacity))
                    .filter(|colony| colony.buffer.as_ref().is_some_and(|buffer| buffer.used_energy_capacity() > ENERGY_THRESHOLD));
                
                let Some(export_colony) = export_colony else { done!(self) };

                Ok(Next(Self::CollectingFrom(export_colony.name)))
            },
            Self::CollectingFrom(room_name) => {
                let colony = colonies.get(&room_name).ok_or(anyhow!("Room is not a colony"))?;
                let buffer = colony.buffer.as_ref().ok_or(anyhow!("{room_name} has no buffer"))?;
                next_if!(buffer.used_energy_capacity() < ENERGY_THRESHOLD, Self::Idle);

                defer!(movement.move_vcreep_to(creep, buffer.pos(), 1), self)?;
                defer_err!(creep.withdraw(*buffer, ResourceType::Energy, None), self)?;
                
                Ok(Next(Self::GoingHome))
            },
            Self::GoingHome => {
                defer!(movement.move_vcreep_to(creep, home.center, 10), self)?;

                Ok(Next(Self::ProvidingIdle))
            },
            Self::ProvidingIdle => {
                next_if!(creep.next_used_energy_capacity() == 0, Self::Idle);

                if let Some(consumer) = coordinator.assign_consumer(creep) {
                    next!(Self::ProvidingTo(consumer))
                }

                if let Some(buffer) = home.buffer.as_ref() {
                    next!(Self::StoringAway(*buffer))
                }

                defer!(movement.move_vcreep_to(creep, home.center, 3), self)?;

                Ok(Done(self))
            },
            Self::ProvidingTo(ref consumer) => {
                let Some(mut task_handle) = coordinator.consumers.heartbeat(consumer, creep.handle()) else { next!(Self::ProvidingIdle) };
                next_if!(creep.next_used_energy_capacity() == 0, Self::finish_task(task_handle));

                defer!(movement.move_vcreep_to(creep, consumer.pos(), 1), self)?;
                task_handle.consume(defer_err!(creep.transfer(consumer.clone(), ResourceType::Energy, None), self)?);

                Self::finish_task(task_handle);
                Ok(Next(Self::ProvidingIdle))
            },
            Self::StoringAway(ref buffer) => {
                next_if!(creep.next_used_energy_capacity() == 0, Self::Idle);

                defer!(movement.move_vcreep_to(creep, buffer.pos(), 1), self)?;
                defer_err!(creep.transfer(*buffer, ResourceType::Energy, None), self)?;

                Ok(Next(Self::Idle))
            },
        }
    }
}