use screeps::{Part, ResourceType, UPGRADE_CONTROLLER_POWER};

use crate::{colony::ColonyView, creeps::virtual_creep::VirtualCreep, domain_traits::EnergyStoreAccessors, statemachine::{Flow, Stop}};

pub fn update(creep: &mut VirtualCreep, home: &ColonyView<'_>) -> Stop {
    let controller = home.controller.clone();
    let container = &home.plan.upgrader_container;

    creep.approach_tugged(container.pos, 0)?;

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