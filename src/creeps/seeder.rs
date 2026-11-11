use screeps::HasPosition;

use crate::{colony::ColonyView, creeps::{gather_energy, virtual_creep::VirtualCreep}, domain_traits::EnergyStoreAccessors, statemachine::{Flow, OrBreak, break_if}};

pub fn transition(creep: &mut VirtualCreep, home: &ColonyView<'_>) -> Flow {
    gather_energy(creep, || home.plan.upgrader_container.resolve().filter(|container| container.used_energy_capacity() > 0).or_break())?;

    creep.approach(home.controller.pos(), 3)?;

    break_if(creep.curr_used_energy_capacity() == 0)?;
    creep.upgrade_controller(home.controller.clone())?;

    Flow::Continue(())
}