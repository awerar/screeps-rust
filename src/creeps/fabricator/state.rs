use anyhow::Result;
use derive_where::derive_where;
use enum_display::EnumDisplay;
use screeps::{HasPosition, ResourceType};
use serde::Deserialize;

use crate::{break_deferable, break_if, break_move, check::Check, colony::ColonyView, creeps::{fabricator::{coordinator::{FabricatorCoordinator, FabricatorTaskHandle}, task::FabricatorTask}, virtual_creep::VirtualCreep}, domain_traits::EnergyStoreAccessors, ids::{CheckState, Checked, Unchecked}, movement::requests::MovementRequests, statemachine::Transition};

// TODO: Expiration
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

    fn finish_task(handle: FabricatorTaskHandle) -> Self {
        handle.remove();
        Self::Idle
    }

    pub fn update(self, creep: &mut VirtualCreep, home: &ColonyView<'_>, movement: &mut MovementRequests, coordinator: &mut FabricatorCoordinator) -> anyhow::Result<Transition<Self>> {
        use Transition::*;

        match self {
            Self::Idle => {
                if let Some(task) = coordinator.assign_task(creep, home) {
                    return Ok(Continue(Self::Performing(task)))
                }

                Ok(Break(self))
            },
            Self::CollectingFor(ref task) => {
                if coordinator.heartbeat(creep, task).is_none() { return Ok(Continue(Self::Idle)) }

                if creep.next_used_energy_capacity() > 0 {
                    return Ok(Continue(Self::Performing(task.clone())))
                }

                let Some(buffer) = &home.buffer else { return Ok(Break(self)) };
                break_deferable!(break_move!(movement.move_vcreep_to(creep, buffer.pos(), 1), self), self)?;

                break_if!(buffer.used_energy_capacity() == 0, self);
                break_if!(creep.outgoing() > 0, self);
                break_deferable!(creep.withdraw(buffer.clone(), ResourceType::Energy, None), self)?;

                Ok(Continue(Self::Performing(task.clone())))
            },
            Self::Performing(ref task) => {
                let Some(mut handle) = coordinator.heartbeat(creep, task) else { return Ok(Continue(Self::Idle)) };

                if creep.next_used_energy_capacity() == 0 {
                    return Ok(Continue(Self::CollectingFor(task.clone())))
                }

                break_deferable!(break_move!(movement.move_vcreep_to(creep, task.pos(home), task.work_range()), self), self)?;

                break_if!(creep.incoming_energy() > 0, self);
                break_deferable!(task.creep_work(creep, home, &mut handle), self)?;

                break_if!(!matches!(&handle, FabricatorTaskHandle::Collab(handle) if handle.reserved() == 0), self);

                Ok(Continue(Self::finish_task(handle)))
            }
        }
    }
}