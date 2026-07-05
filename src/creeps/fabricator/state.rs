use anyhow::Result;
use derive_where::derive_where;
use enum_display::EnumDisplay;
use screeps::{Creep, HasPosition, ResourceType, SharedCreepProperties};
use serde::Deserialize;

use crate::{check::Check, colony::ColonyView, creeps::fabricator::{coordinator::FabricatorCoordinator, task::FabricatorTask}, domain_traits::{EnergyStoreAccessors, Withdrawable}, ids::{CheckState, Checked, IntoHandle, Unchecked, WithId}, movement::requests::MovementRequests, statemachine::Transition};

#[derive(Debug, Default, EnumDisplay)]
#[derive_where(Serialize, Deserialize, Clone; FabricatorTask<S>)]
pub enum FabricatorCreep<S: CheckState = Checked> {
    #[default] Idle,
    CollectingFor(FabricatorTask<S>),
    Performing(FabricatorTask<S>)
}

impl<'de> Deserialize<'de> for FabricatorCreep {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let us = FabricatorCreep::<Unchecked>::deserialize(deserializer)?;
        Ok(match us {
            FabricatorCreep::Idle => Self::Idle,
            FabricatorCreep::CollectingFor(task) => 
                task.check().map_or(FabricatorCreep::Idle, FabricatorCreep::CollectingFor),
            FabricatorCreep::Performing(task) => 
                task.check().map_or(FabricatorCreep::Idle, FabricatorCreep::Performing),
        })
    }
}

impl FabricatorCreep {
    pub fn is_consumer(&self) -> bool { matches!(self, Self::CollectingFor(_) | Self::Performing(_)) }
    pub fn is_provider(&self) -> bool { matches!(self, Self::Idle) }

    pub fn update(self, creep: &WithId<Creep>, home: &ColonyView<'_>, movement: &mut MovementRequests, coordinator: &mut FabricatorCoordinator) -> anyhow::Result<Transition<Self>> {
        use Transition::*;

        match self {
            Self::Idle => {
                let task = coordinator.assign_task(creep);
                if let Some(task) = task {
                    return Ok(Continue(Self::Performing(task)))
                }

                Ok(Break(self))
            },
            Self::CollectingFor(ref task) => {
                if task.has_timed_out() || !coordinator.heartbeat_task(&creep.clone().handle(), task) { return Self::fail_task(creep, task, coordinator) }

                if creep.used_energy_capacity() > 0 {
                    return Ok(Continue(Self::Performing(task.clone())))
                }

                let Some(buffer) = &home.buffer else { return Ok(Break(self)) };
                if buffer.used_energy_capacity() == 0 { return Ok(Break(self)) }

                if movement.move_creep_to(creep, buffer.pos(), 1).in_range() {
                    creep.withdraw(buffer.withdrawable(), ResourceType::Energy, None)?;
                    return Ok(Break(Self::Performing(task.clone())))
                }
                    
                Ok(Break(self))
            },
            Self::Performing(ref task) => {
                if task.has_timed_out() || !coordinator.heartbeat_task(&creep.clone().handle(), task) { return Self::fail_task(creep, task, coordinator) }

                let creep_energy = creep.used_energy_capacity();
                if creep_energy == 0 {
                    return Ok(Continue(Self::CollectingFor(task.clone())))
                }

                if movement.move_creep_to(creep, task.pos(), task.work_range()).in_range() && creep_energy > 0 {
                    task.creep_work(creep)?;
                }

                Ok(Break(self))
            }
        }
    }

    #[expect(clippy::unnecessary_wraps)]
    fn fail_task(creep: &WithId<Creep>, task: &FabricatorTask, coordinator: &mut FabricatorCoordinator) -> anyhow::Result<Transition<Self>> {
        coordinator.finish_task(&creep.clone().handle(), task, false);
        Ok(Transition::Continue(FabricatorCreep::Idle))
    }
}