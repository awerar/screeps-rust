use std::{collections::HashMap, ops::Deref};

use derive_deref::Deref;
use derive_where::derive_where;
use enum_display::EnumDisplay;
use screeps::{HasPosition, ResourceType, RoomName};
use serde::Deserialize;
use anyhow::Result;

use crate::{check::Check, colony::{Colonies, ColonyBuffer, ColonyView, steps::ColonyStep}, coordination::allocations::CreepAllocationHandle, creeps::{truck::{TruckCoordinator, stop::ConsumerTruckStop}, virtual_creep::VirtualCreep}, defer, defer_err, domain_traits::EnergyStoreAccessors, done, ids::{CheckState, Checked, Unchecked}, movement::requests::MovementRequests, next, next_if, statemachine::Transition};

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

impl ImportTruckState {
    fn finish_task(task_handle: CreepAllocationHandle<'_>) -> Self {
        task_handle.release();
        Self::ProvidingIdle
    }

    pub fn update<'a>(self, creep: &mut VirtualCreep, home: &ColonyView<'a>, colonies: &HashMap<RoomName, ExportColonyView<'a>>, movement: &mut MovementRequests, coordinator: &mut TruckCoordinator) -> Result<Transition<Self>> {
        use Transition::*;

        match self {
            Self::Idle => {
                let export_colony = colonies.values().max_by_key(|colony| colony.buffer.used_energy_capacity());
                let Some(export_colony) = export_colony else { done!(self) };

                Ok(Next(Self::CollectingFrom(export_colony.name)))
            },
            Self::CollectingFrom(room_name) => {
                let Some(colony) = colonies.get(&room_name) else { next!(Self::Idle) };

                defer!(movement.move_vcreep_to(creep, colony.buffer.pos(), 1), self)?;
                defer_err!(creep.withdraw(colony.buffer, ResourceType::Energy, None), self)?;
                
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