use std::mem;

use enum_display::EnumDisplay;
use screeps::{Part, ResourceType, UPGRADE_CONTROLLER_POWER};
use serde::{Deserialize, Serialize};

use crate::{goto, colony::ColonyView, creeps::virtual_creep::VirtualCreep, domain_traits::EnergyStoreAccessors, statemachine::{Flow, Next, step}};

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Copy, EnumDisplay, Default)]
pub enum TerraformerCreep {
    #[default]
    Going,
    Upgrading
}

impl TerraformerCreep {
    pub fn update(&mut self, creep: &mut VirtualCreep, home: &ColonyView<'_>) -> anyhow::Result<()> {
        *self = step(mem::take(self), |state| state.transition(creep, home))?;
        Ok(())
    }

    fn transition(self, creep: &mut VirtualCreep, home: &ColonyView<'_>) -> Flow<Next<Self>> {
        use TerraformerCreep::*;

        let controller = home.controller.clone();
        let container = &home.plan.upgrader_container;

        match self {
            Going => {
                creep.approach_tugged(container.pos, 0)?;
                goto!(Upgrading)
            },
            Upgrading => {
                if creep.curr_used_energy_capacity() > 0 {
                    creep.upgrade_controller(controller)?;
                }

                if let Some(container) = container.resolve() 
                    && container.used_energy_capacity() > 0
                    && creep.next_used_energy_capacity() < creep.body().part_count(Part::Work) as u32 * UPGRADE_CONTROLLER_POWER {
                        creep.withdraw(container, ResourceType::Energy, None)?;
                }

                Flow::Break
            }
        }
    }
}