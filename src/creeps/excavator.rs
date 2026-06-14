use anyhow::{Ok, anyhow};
use enum_display::EnumDisplay;
use log::warn;
use screeps::{ConstructionSite, Creep, HasId, Part, ResourceType, SharedCreepProperties, Source, StructureContainer, StructureExtension, StructureLink, StructureSpawn, Transferable};
use serde::{Deserialize, Serialize};

use crate::{colony::{ColonyView, planning::{plan::SourcePlan, planned_ref::{PlannedStructureRef, ResolvableSiteRef, ResolvableStructureRef}}}, movement::requests::MovementRequests, safeid::SafeID, statemachine::{StateMachine, Transition}, utils::EnergyStore};

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, EnumDisplay, Default)]
pub enum ExcavatorCreep {
    #[default]
    Going,
    Mining
}

trait SourceFillTarget: Transferable + screeps::HasStore {}
impl SourceFillTarget for StructureSpawn {}
impl SourceFillTarget for StructureLink {}
impl SourceFillTarget for StructureExtension {}

impl SourcePlan {
    pub fn get_construction_site(&self) -> Option<ConstructionSite> {
        if let site@Some(_) = self.container.resolve_site() { return site; }
        if let site@Some(_) = self.link.resolve_site() { return site; }
        if let site@Some(_) = self.spawn.resolve_site() { return site; }
        self.extensions.iter().find_map(PlannedStructureRef::resolve_site)
    }

    fn get_fill_target(&self) -> Option<Box<dyn SourceFillTarget>> {
        let mut fillables = Vec::new();

        fillables.extend(self.spawn.resolve().map(|x| Box::new(x) as Box<dyn SourceFillTarget>));
        fillables.extend(self.extensions.resolve().into_iter().map(|x| Box::new(x) as Box<dyn SourceFillTarget>));
        fillables.extend(self.link.resolve().map(|x| Box::new(x) as Box<dyn SourceFillTarget>));

        fillables.into_iter()
            .find(|fillable| fillable.store().get_free_capacity(Some(ResourceType::Energy)) > 0)
    }

    fn get_energy_destination(&self) -> Option<EnergyDestination> {
        self.get_construction_site().map(EnergyDestination::ConstructionSite)
            .or_else(|| self.get_fill_target().map(EnergyDestination::FillTarget))
            .or_else(|| self.container.resolve().filter(|container| container.store().free_energy_capacity() > 0).map(EnergyDestination::Container))
    }
}

enum EnergyDestination {
    ConstructionSite(ConstructionSite),
    FillTarget(Box<dyn SourceFillTarget>),
    Container(StructureContainer)
}

impl EnergyDestination {
    fn can_also_harvest(&self) -> bool {
        !matches!(self, Self::ConstructionSite(_))
    }

    fn recieve(&self, creep: &SafeID<Creep>) {
        match self {
            EnergyDestination::ConstructionSite(site) => 
                creep.build(site).ok(),
            EnergyDestination::FillTarget(target) => 
                creep.transfer(&**target, ResourceType::Energy, None).ok(),
            EnergyDestination::Container(container) => 
                creep.transfer(container, ResourceType::Energy, None).ok(),
            
        };
    }
}

fn work_count(creep: &SafeID<Creep>) -> u32 {
    creep.body().iter().filter(|bodypart| bodypart.part() == Part::Work).count() as u32
}

/*
    Should always try be as full as possible
    Should always do as big outputs as possible

    Should only output energy if will overflow


    Should always mine if possible
    Should only build 
*/

type Args<'a> = (SafeID<Source>, ColonyView<'a>, &'a mut MovementRequests);
impl StateMachine<SafeID<Creep>, Args<'_>> for ExcavatorCreep {
    fn update(self, creep: &SafeID<Creep>, args: &mut Args<'_>) -> anyhow::Result<Transition<Self>> {
        use ExcavatorCreep::*;
        use Transition::*;

        let (source, home, movement) = args;

        let plan = home.plan.sources.get(&source.id()).ok_or(anyhow!("Plan doesn't exist"))?;

        match self {
            Going => {
                let harvest_pos = plan.container.as_ref().ok_or(anyhow!("No container"))?.pos;
                if movement.move_tugged_to(creep, harvest_pos, 0).in_range() {
                    return Ok(Continue(Mining))
                }

                Ok(Break(self))
            },
            Mining => {
                let Some(energy_dest) = plan.get_energy_destination() else {
                    warn!("{} has nowhere to put its energy", creep.name());
                    return Ok(Break(self));
                };

                let harvest_energy = (work_count(creep) * 2).try_into().unwrap();

                let mut can_harvest = true;
                if creep.store().free_energy_capacity() < harvest_energy {
                    energy_dest.recieve(creep);

                    if !energy_dest.can_also_harvest() {
                        can_harvest = false;
                    }
                }

                if can_harvest {
                    creep.harvest(&**source).ok();
                }

                if !matches!(energy_dest, EnergyDestination::Container(_))
                    && let Some(container) = plan.container.resolve() {
                        // Maybes makes creep drop harvested resources, which will just put into the container anyway
                        creep.withdraw(&container, ResourceType::Energy, None).ok();
                    }

                Ok(Break(self))
            }
        }
    }
}