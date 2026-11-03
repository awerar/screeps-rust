use derive_where::derive_where;
use enum_display::EnumDisplay;
use screeps::{CARRY_CAPACITY, CONTAINER_CAPACITY, HasPosition, Part, ResourceType};
use serde::Deserialize;

use crate::{check::Check, colony::{ColonyBuffer, ColonyView}, creeps::virtual_creep::VirtualCreep, defer, domain_traits::{EnergyStoreAccessors, ResolvableId}, ids::{CheckState, Checked, Unchecked}, movement::requests::MovementRequests, next, next_if, statemachine::Transition, structure::ProviderStructure};

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
    pub fn update(self, creep: &mut VirtualCreep, home: &ColonyView<'_>, movement: &mut MovementRequests) -> anyhow::Result<Transition<Self>> {
        use SeederCreep::*;
        use Transition::*;

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
                        match buffer {
                            ColonyBuffer::Container(container) => 
                                next!(Refilling(ProviderStructure::new(container.resolve()))),
                            ColonyBuffer::Storage(storage) => 
                                next!(Refilling(ProviderStructure::new(storage.resolve()))),
                        }
                }

                Ok(Done(Idle))
            },
            Refilling(target) => {
                next_if!(creep.curr_used_energy_capacity() > 0, Upgrading);
                next_if!(target.used_energy_capacity() == 0, Idle);

                defer!(movement.move_vcreep_to(creep, target.pos(), 1), self)?;
                creep.withdraw(target, ResourceType::Energy, None)?;

                next!(Upgrading)
            },
            Self::Upgrading => {
                next_if!(creep.curr_used_energy_capacity() == 0, Idle);

                defer!(movement.move_vcreep_to(creep, home.controller.pos(), 3), self)?;
                creep.upgrade_controller(home.controller.clone())?;

                Ok(Done(self))
            }
        }
    }
}