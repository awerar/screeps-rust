use enum_display::EnumDisplay;
use screeps::{Part, ResourceType, UPGRADE_CONTROLLER_POWER};
use serde::{Deserialize, Serialize};

use crate::{colony::ColonyView, creeps::virtual_creep::VirtualCreep, defer, domain_traits::EnergyStoreAccessors, movement::requests::MovementRequests, statemachine::Transition};

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, EnumDisplay, Default)]
pub enum TerraformerCreep {
    #[default]
    Going,
    Upgrading
}

impl TerraformerCreep {
    pub fn update(self, creep: &mut VirtualCreep, home: &ColonyView<'_>, movement: &mut MovementRequests) -> anyhow::Result<Transition<Self>> {
        use TerraformerCreep::*;
        use Transition::*;

        let controller = home.controller.clone();
        let container = &home.plan.upgrader_container;

        match self {
            Going => {
                defer!(movement.move_vtugged_to(creep, container.pos, 0), self)?;

                Ok(Next(Upgrading))
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

                Ok(Done(self))
            }
        }
    }
}