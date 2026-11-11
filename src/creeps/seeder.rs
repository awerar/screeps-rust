use std::mem;

use derive_where::derive_where;
use enum_display::EnumDisplay;
use screeps::{CARRY_CAPACITY, CONTAINER_CAPACITY, HasPosition, Part};
use serde::Deserialize;

use crate::{check::Check, colony::ColonyView, creeps::{gather_energy, virtual_creep::VirtualCreep}, domain_traits::EnergyStoreAccessors, ids::{CheckState, Checked, Unchecked}, goto, statemachine::{Flow, step, break_if}, structure::ProviderStructure};

#[derive(Debug, PartialEq, Eq, EnumDisplay, Default)]
#[derive_where(Serialize, Deserialize, Clone; ProviderStructure<S>, S)]
pub enum SeederCreep<S: CheckState = Checked> {
    #[default]
    Idle,
    Refilling(ProviderStructure<S>),
    Upgrading
}

impl<'de> Deserialize<'de> for SeederCreep {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let us = SeederCreep::<Unchecked>::deserialize(deserializer)?;
        Ok(match us {
            SeederCreep::Idle => Self::Idle,
            SeederCreep::Upgrading => Self::Upgrading,
            SeederCreep::Refilling(provider) => provider.check().map_or_default(Self::Refilling),
        })
    }
}

impl SeederCreep {
    pub fn update(&mut self, creep: &mut VirtualCreep, home: &ColonyView<'_>) -> anyhow::Result<()> {
        *self = step(mem::take(self), |state| state.transition(creep, home))?;
        Ok(())
    }

    fn transition(self, creep: &mut VirtualCreep, home: &ColonyView<'_>) -> Flow<Self> {
        use SeederCreep::*;

        match self {
            Idle => {
                goto!(if creep.next_used_energy_capacity() > 0 { Upgrading });
                
                let min_refill = (creep.body().part_count(Part::Carry) as u32 * CARRY_CAPACITY).min(CONTAINER_CAPACITY);
                
                let container = home.plan.upgrader_container.resolve().filter(|c| c.used_energy_capacity() > min_refill);
                goto!(if let Some(container) = container { Refilling(ProviderStructure::new(container)) });

                let buffer = home.buffer.filter(|b| b.used_energy_capacity() > min_refill);
                goto!(if let Some(buffer) = buffer { Refilling(ProviderStructure::new(buffer)) });

                Flow::Break
            },
            Refilling(target) => {
                gather_energy(creep, || Flow::Continue(target))?;
                Flow::Continue(Upgrading)
            },
            Self::Upgrading => {
                goto!(if creep.next_used_energy_capacity() == 0 { Idle });
                break_if(creep.curr_used_energy_capacity() == 0)?;

                creep.approach(home.controller.pos(), 3)?;
                creep.upgrade_controller(home.controller.clone())?;

                Flow::Break
            }
        }
    }
}