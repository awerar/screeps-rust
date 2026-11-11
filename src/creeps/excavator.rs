use anyhow::anyhow;
use log::warn;
use screeps::{ConstructionSite, HasId, Part, Position, REPAIR_POWER, ResourceType, Source, StructureContainer, StructureExtension, StructureLink, StructureSpawn};

use crate::{colony::{ColonyView, plan::{SourcePlan, refs::PlannedStructureRef}}, creeps::virtual_creep::{IntentType, VirtualCreep}, domain_traits::{EnergyStoreAccessors, HasHits, ObjectId}, statemachine::{Flow, Stop}, structure::RepairableStructure};

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
    
    pub fn get_repair_target(&self) -> Option<RepairableStructure> {
        self.container.resolve().map(RepairableStructure::new)
            .map(|structure| (structure, structure.hits_max() - structure.hits()))
            .filter(|(_, missing_hits)| *missing_hits > REPAIR_POWER * 10) // TODO: Improve this
            .map(|(structure, _)| structure)
    }

    fn get_energy_destination(&self) -> Option<EnergyDestination> {
        self.get_repair_target().map(EnergyDestination::Repair)
            .or_else(|| self.get_construction_site().map(EnergyDestination::ConstructionSite))
            .or_else(|| self.get_fill_target())
            .or_else(|| self.container.resolve().filter(|container| container.free_energy_capacity() > 0).map(EnergyDestination::Container))
    }
}

enum EnergyDestination {
    Repair(RepairableStructure),
    ConstructionSite(ConstructionSite),
    Spawn(StructureSpawn),
    Extension(StructureExtension),
    Link(StructureLink),
    Container(StructureContainer)
}

impl EnergyDestination {
    fn can_also_harvest(&self) -> bool {
        !matches!(self, Self::ConstructionSite(_) | Self::Repair(_))
    }

    fn recieve(self, creep: &mut VirtualCreep) -> Flow<u32> {
        match self {
            EnergyDestination::Repair(structure) => 
                creep.repair(structure),
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

pub fn is_in_position(creep_pos: Position, source: &ObjectId<Source>, home: &ColonyView) -> bool {
    home.plan.sources.get(&source.screeps_id()).is_some_and(|plan| plan.container.pos == creep_pos)
}

pub fn update(creep: &mut VirtualCreep, source: &Source, home: &ColonyView<'_>) -> Stop {
    let plan = home.plan.sources.get(&source.id()).ok_or(anyhow!("Source plan doesn't exist"))?;

    creep.approach_tugged(plan.container.pos, 0)?;

    let harvest_energy = (creep.body().part_count(Part::Work) * 2) as u32;
    let target_energy = creep.capacity() - harvest_energy;

    let mut transferring_to_container = false;
    if creep.next_used_energy_capacity() > target_energy {
        let Some(energy_dest) = plan.get_energy_destination() else {
            creep.cancel_intent(IntentType::Harvest);

            warn!("{} has nowhere to put its energy", creep.name());
            return Flow::Break;
        };

        if !energy_dest.can_also_harvest() {
            creep.cancel_intent(IntentType::Harvest);
        }

        transferring_to_container = matches!(energy_dest, EnergyDestination::Container(_));
        energy_dest.recieve(creep)?;
    }

    if transferring_to_container { return Flow::Break }

    let container: Option<StructureContainer> = plan.container.resolve();
    let Some(container) = container else { return Flow::Break };

    let defecit = target_energy.saturating_sub(creep.next_used_energy_capacity());
    let defecit = defecit.min(container.used_energy_capacity()).min(creep.curr_free_capacity());
    if defecit == 0 { return Flow::Break }

    creep.withdraw(container, ResourceType::Energy, Some(defecit))?;
    Flow::Break
}