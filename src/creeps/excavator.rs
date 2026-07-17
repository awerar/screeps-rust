use anyhow::{anyhow};
use enum_display::EnumDisplay;
use log::warn;
use screeps::{ConstructionSite, HasId, Part, ResourceType, Source, StructureContainer, StructureExtension, StructureLink, StructureSpawn};
use serde::{Deserialize, Serialize};

use crate::{colony::{ColonyView, planning::{plan::SourcePlan, planned_ref::{PlannedStructureRef, ResolvableSiteRef, ResolvableStructureRef}}}, creeps::virtual_creep::{IntentError, IntentType, VirtualCreep}, defer, domain_traits::EnergyStoreAccessors, movement::requests::MovementRequests, statemachine::Transition};

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, EnumDisplay, Default)]
pub enum ExcavatorCreep {
    #[default]
    Going,
    Mining
}

impl SourcePlan {
    pub fn get_construction_site(&self) -> Option<ConstructionSite> {
        if let Some(site) = self.container.resolve_site() { return Some(site); }
        if let Some(site) = self.link.resolve_site() { return Some(site); }
        if let Some(site) = self.spawn.resolve_site() { return Some(site); }
        self.extensions.iter().find_map(PlannedStructureRef::resolve_site)
    }

    fn get_fill_target(&self) -> Option<EnergyDestination> {
        let mut fillable = None;

        fillable = fillable.or_else(|| self.spawn.resolve().filter(|x| x.free_energy_capacity() > 0).map(EnergyDestination::Spawn));
        fillable = fillable.or_else(|| self.extensions.resolve().into_iter().filter(|x| x.free_energy_capacity() > 0).map(EnergyDestination::Extension).next());
        fillable = fillable.or_else(|| self.link.resolve().filter(|x| x.free_energy_capacity() > 0).map(EnergyDestination::Link));

        fillable
    }

    fn get_energy_destination(&self) -> Option<EnergyDestination> {
        self.get_construction_site().map(EnergyDestination::ConstructionSite)
            .or_else(|| self.get_fill_target())
            .or_else(|| self.container.resolve().filter(|container| container.free_energy_capacity() > 0).map(EnergyDestination::Container))
    }
}

enum EnergyDestination {
    ConstructionSite(ConstructionSite),
    Spawn(StructureSpawn),
    Extension(StructureExtension),
    Link(StructureLink),
    Container(StructureContainer)
}

impl EnergyDestination {
    fn can_also_harvest(&self) -> bool {
        !matches!(self, Self::ConstructionSite(_))
    }

    fn recieve(self, creep: &mut VirtualCreep) -> Result<u32, IntentError> {
        match self {
            EnergyDestination::ConstructionSite(site) => 
                creep.build(site.clone()),
            EnergyDestination::Spawn(spawn) => 
                creep.transfer(spawn, ResourceType::Energy, None),
            EnergyDestination::Extension(extension) => 
                creep.transfer(extension, ResourceType::Energy, None),
            EnergyDestination::Link(link) => 
                creep.transfer(link, ResourceType::Energy, None),
            EnergyDestination::Container(container) => 
                creep.transfer(container, ResourceType::Energy, None),   
        }
    }
}

/*
    Should always try be as full as possible
    Should always do as big outputs as possible

    Should only output energy if will overflow


    Should always mine if possible
    Should only build 
*/

impl ExcavatorCreep {
    pub fn update(self, creep: &mut VirtualCreep, source: &Source, home: &ColonyView<'_>, movement: &mut MovementRequests) -> anyhow::Result<Transition<Self>> {
        use ExcavatorCreep::*;
        use Transition::*;

        let plan = home.plan.sources.get(&source.id()).ok_or(anyhow!("Plan doesn't exist"))?;

        match self {
            Going => {
                let harvest_pos = plan.container.as_ref().ok_or(anyhow!("No container"))?.pos;
                defer!(movement.move_vtugged_to(creep, harvest_pos, 0), self)?;

                Ok(Next(Mining))
            },
            Mining => {
                creep.harvest_source(source.clone())?;

                let harvest_energy = (creep.body().part_count(Part::Work) * 2) as u32;
                let target_energy = creep.capacity() - harvest_energy;

                let mut transferring_to_container = false;
                if creep.next_used_energy_capacity() > target_energy {
                    let Some(energy_dest) = plan.get_energy_destination() else {
                        creep.cancel_intent(IntentType::Harvest);

                        warn!("{} has nowhere to put its energy", creep.name());
                        return Ok(Done(self));
                    };

                    if !energy_dest.can_also_harvest() {
                        creep.cancel_intent(IntentType::Harvest);
                    }

                    transferring_to_container = matches!(energy_dest, EnergyDestination::Container(_));
                    energy_dest.recieve(creep)?;
                }

                if transferring_to_container { return Ok(Done(self)) }

                let container: Option<StructureContainer> = plan.container.resolve();
                let Some(container) = container else { return Ok(Done(self)) };

                let defecit = target_energy.saturating_sub(creep.next_used_energy_capacity());
                let defecit = defecit.min(container.used_energy_capacity()).min(creep.curr_free_capacity());
                if defecit == 0 { return Ok(Done(self)) }

                creep.withdraw(container, ResourceType::Energy, Some(defecit))?;
                Ok(Done(self))
            }
        }
    }
}