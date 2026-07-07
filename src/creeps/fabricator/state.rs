use anyhow::Result;
use derive_where::derive_where;
use enum_display::EnumDisplay;
use screeps::{HasPosition, ResourceType};
use serde::Deserialize;

use crate::{break_deferable, break_if, break_move, brk, check::Check, colony::ColonyView, cont, continue_if, creeps::{fabricator::{coordinator::FabricatorCoordinator, task::{FabricatorTask, StructureTask}}, virtual_creep::VirtualCreep}, domain_traits::EnergyStoreAccessors, ids::{CheckState, Checked, Unchecked}, movement::requests::MovementRequests, statemachine::Transition};

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
                    cont!(Self::Performing(task))
                }

                Ok(Break(self))
            },
            Self::CollectingFor(ref task) => {
                continue_if!(coordinator.refresh(creep, task).is_none(), Self::Idle);
                continue_if!(creep.next_used_energy_capacity() > 0, Self::Performing(task.clone()));

                let Some(buffer) = &home.buffer else { brk!(self) };
                break_deferable!(break_move!(movement.move_vcreep_to(creep, buffer.pos(), 1), self), self)?;

                break_if!(buffer.used_energy_capacity() == 0, self);
                break_if!(creep.outgoing() > 0, self);
                break_deferable!(creep.withdraw(buffer.clone(), ResourceType::Energy, None), self)?;

                Ok(Continue(Self::Performing(task.clone())))
            },
            Self::Performing(ref task) => {
                continue_if!(creep.next_used_energy_capacity() == 0, Self::CollectingFor(task.clone()));

                match task {
                    FabricatorTask::Structure(task) => {
                        let Some(mut handle) = coordinator.refresh_structure(creep, task) else { cont!(Self::Idle) };

                        break_deferable!(break_move!(movement.move_vcreep_to(creep, task.pos(), 1), self), self)?;

                        break_if!(creep.curr_used_energy_capacity() == 0, self);
                        handle.consume(break_deferable!(task.creep_work(creep), self)?);

                        break_if!(handle.reserved() > 0, self);

                        handle.release();
                        Ok(Continue(Self::Idle))
                    },
                    FabricatorTask::Upgrading => {
                        continue_if!(coordinator.upgrade.refresh(creep.handle()).is_none(), Self::Idle);

                        break_deferable!(break_move!(movement.move_vcreep_to(creep, home.controller.pos(), 3), self), self)?;

                        break_if!(creep.curr_used_energy_capacity() == 0, self);
                        break_deferable!(creep.upgrade_controller(home.controller.clone()), self)?;

                        Ok(Break(self))
                    }
                }
            }
        }
    }
}