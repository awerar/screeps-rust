use derive_where::derive_where;
use enum_display::EnumDisplay;
use screeps::{CARRY_CAPACITY, CONTAINER_CAPACITY, HasPosition, Part};
use serde::Deserialize;

use crate::{check::Check, colony::ColonyView, creeps::{gather_energy, virtual_creep::VirtualCreep}, domain_traits::EnergyStoreAccessors, ids::{CheckState, Checked, Unchecked}, movement::requests::MovementRequests, next, next_if, statemachine::{Step, step, Wait::Ready, wait_if}, structure::ProviderStructure};

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
    pub fn update(&mut self, creep: &mut VirtualCreep, home: &ColonyView<'_>, movement: &mut MovementRequests) {
        step(self, |state| state.transition(creep, home, movement));
    }

    fn transition(&mut self, creep: &mut VirtualCreep, home: &ColonyView<'_>, movement: &mut MovementRequests) -> Step<Self> {
        use SeederCreep::*;

        match self {
            Idle => {
                if creep.next_used_energy_capacity() > 0 { next!(Upgrading) }
                
                let min_refill = (creep.body().part_count(Part::Carry) as u32 * CARRY_CAPACITY).min(CONTAINER_CAPACITY);
                
                if let Some(container) = home.plan.upgrader_container.resolve()
                    && container.used_energy_capacity() > min_refill {
                        next!(Refilling(ProviderStructure::new(container)));
                }

                if let Some(buffer) = home.buffer 
                    && buffer.used_energy_capacity() > min_refill {
                        next!(Refilling(ProviderStructure::new(buffer)))
                }

                Step::Stay
            },
            Refilling(target) => {
                gather_energy(creep, movement, || Ready(*target))?;
                next!(Upgrading)
            },
            Self::Upgrading => {
                next_if!(creep.next_used_energy_capacity() == 0, Idle);
                wait_if(creep.curr_used_energy_capacity() == 0)?;

                movement.approach(creep, home.controller.pos(), 3)?;
                creep.upgrade_controller(home.controller.clone())?;

                Step::Stay
            }
        }
    }
}