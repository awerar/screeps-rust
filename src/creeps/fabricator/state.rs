use anyhow::Result;
use derive_where::derive_where;
use enum_display::EnumDisplay;
use screeps::{HasPosition, ResourceType};
use serde::Deserialize;

use crate::{break_deferable, break_move, check::Check, colony::ColonyView, creeps::{fabricator::{coordinator::FabricatorCoordinator, task::FabricatorTask}, virtual_creep::VirtualCreep}, domain_traits::EnergyStoreAccessors, ids::{CheckState, Checked, Unchecked}, movement::requests::MovementRequests, statemachine::Transition};

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

    // TODO: Check transitions
    pub fn update(self, creep: &mut VirtualCreep, home: &ColonyView<'_>, movement: &mut MovementRequests, coordinator: &mut FabricatorCoordinator) -> anyhow::Result<Transition<Self>> {
        use Transition::*;

        match self {
            Self::Idle => {
                let task = coordinator.assign_task(creep);
                if let Some(task) = task {
                    return Ok(Continue(Self::Performing(task)))
                }

                Ok(Continue(self))
            },
            Self::CollectingFor(ref task) => {
                if task.has_timed_out() || !coordinator.heartbeat_task(&creep.handle(), task) { return Self::fail_task(creep, task, coordinator) }

                if creep.next_used_energy_capacity() > 0 {
                    return Ok(Continue(Self::Performing(task.clone())))
                }

                let Some(buffer) = &home.buffer else { return Ok(Break(self)) };
                if buffer.used_energy_capacity() == 0 { return Ok(Break(self)) }

                break_deferable!(break_move!(movement.move_vcreep_to(creep, buffer.pos(), 1), self), self)?;
                break_deferable!(creep.withdraw(buffer.clone(), ResourceType::Energy, None), self)?;

                Ok(Continue(Self::Performing(task.clone())))
            },
            Self::Performing(ref task) => {
                if task.has_timed_out() || !coordinator.heartbeat_task(&creep.handle(), task) { return Self::fail_task(creep, task, coordinator) }

                if creep.next_used_energy_capacity() == 0 {
                    return Ok(Continue(Self::CollectingFor(task.clone())))
                }

                break_deferable!(break_move!(movement.move_vcreep_to(creep, task.pos(), task.work_range()), self), self)?;
                break_deferable!(task.creep_work(creep), self)?;

                Ok(Continue(self))
            }
        }
    }

    #[expect(clippy::unnecessary_wraps)]
    fn fail_task(creep: &mut VirtualCreep, task: &FabricatorTask, coordinator: &mut FabricatorCoordinator) -> anyhow::Result<Transition<Self>> {
        coordinator.finish_task(&creep.handle(), task, false);
        Ok(Transition::Continue(FabricatorCreep::Idle))
    }
}