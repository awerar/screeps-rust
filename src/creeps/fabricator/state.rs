use anyhow::Result;
use derive_where::derive_where;
use enum_display::EnumDisplay;
use screeps::{HasPosition, ResourceType};
use serde::Deserialize;

use crate::{check::Check, colony::ColonyView, creeps::{fabricator::{coordinator::FabricatorCoordinator, task::{FabricatorTask, StructureTask}}, virtual_creep::VirtualCreep}, defer, defer_err, domain_traits::EnergyStoreAccessors, done, done_if, ids::{CheckState, Checked, Unchecked}, movement::requests::MovementRequests, next, next_if, statemachine::Transition};

// TODO: Expiration
#[derive(Debug, Default, EnumDisplay)]
#[derive_where(Serialize, Deserialize, Clone; StructureTask<S>)]
pub enum FabricatorCreep<S: CheckState = Checked> {
    #[default] Idle,
    CollectingFor(FabricatorTask<S>),
    Performing(FabricatorTask<S>),
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

    pub fn update(self, creep: &mut VirtualCreep, home: &ColonyView<'_>, movement: &mut MovementRequests, coordinator: &mut FabricatorCoordinator) -> anyhow::Result<Transition<Self>> {
        use Transition::*;

        match self {
            Self::Idle => {
                if let Some(task) = coordinator.assign_task(creep, home) {
                    next!(Self::Performing(task))
                }

                Ok(Done(self))
            },
            Self::CollectingFor(ref task) => {
                next_if!(coordinator.refresh(creep, task).is_none(), Self::Idle);
                next_if!(creep.next_used_energy_capacity() > 0, Self::Performing(task.clone()));

                let Some(buffer) = &home.buffer else { done!(self) };
                defer!(movement.move_vcreep_to(creep, buffer.pos(), 1), self)?;

                done_if!(buffer.used_energy_capacity() == 0, self);
                done_if!(creep.outgoing() > 0, self);
                defer_err!(creep.withdraw(buffer.clone(), ResourceType::Energy, None), self)?;

                Ok(Next(Self::Performing(task.clone())))
            },
            Self::Performing(ref task) => {
                next_if!(creep.next_used_energy_capacity() == 0, Self::CollectingFor(task.clone()));

                match task {
                    FabricatorTask::Structure(task) => {
                        let Some(mut handle) = coordinator.refresh_structure(creep, task) else { next!(Self::Idle) };

                        defer!(movement.move_vcreep_to(creep, task.pos(), 1), self)?;

                        done_if!(creep.curr_used_energy_capacity() == 0, self);
                        handle.consume(defer_err!(task.creep_work(creep), self)?);

                        done_if!(handle.reserved() > 0, self);

                        handle.release();
                        Ok(Next(Self::Idle))
                    },
                    FabricatorTask::Upgrading => {
                        next_if!(coordinator.upgrade.refresh(creep.handle()).is_none(), Self::Idle);

                        defer!(movement.move_vcreep_to(creep, home.controller.pos(), 3), self)?;

                        done_if!(creep.curr_used_energy_capacity() == 0, self);
                        defer_err!(creep.upgrade_controller(home.controller.clone()), self)?;

                        Ok(Done(self))
                    }
                }
            }
        }
    }
}