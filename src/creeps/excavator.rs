use anyhow::anyhow;
use screeps::{ConstructionSite, ENERGY_REGEN_TIME, HasId, HasStore, Part, Position, REPAIR_POWER, ResourceType, Source, Store, StructureContainer, StructureExtension, StructureLink, StructureSpawn};

use crate::{colony::{ColonyView, plan::{SourcePlan, refs::PlannedStructureRef}}, creeps::virtual_creep::{IntentType, VirtualCreep}, domain_traits::{EnergyStoreAccessors, HasHits, ObjectId, Transferable}, statemachine::{Flow, OrBreak, Stop, break_if},structure::RepairableStructure};

impl SourcePlan {
    pub fn get_construction_site(&self) -> Option<ConstructionSite> {
        if let Some(site) = self.container.resolve_site() { return Some(site); }
        if let Some(site) = self.link.resolve_site() { return Some(site); }
        if let Some(site) = self.spawn.resolve_site() { return Some(site); }
        self.extensions.iter().find_map(PlannedStructureRef::resolve_site)
    }
    
    pub fn get_repair_target(&self) -> Option<RepairableStructure> {
        self.container.resolve().map(RepairableStructure::new)
            .map(|structure| (structure, structure.hits_max() - structure.hits()))
            .filter(|(_, missing_hits)| *missing_hits > REPAIR_POWER * 10) // TODO: Improve this
            .map(|(structure, _)| structure)
    }

    fn get_work(&self) -> Option<Work> {
        self.get_repair_target().map(Work::Repair)
            .or_else(|| self.get_construction_site().map(Work::Build))
    }

    fn get_dump(&self) -> Option<Dump> {
        self.link.resolve().filter(|link| link.free_energy_capacity() > 0).map(Dump::Link)
            .or_else(|| self.container.resolve().filter(|container| container.free_energy_capacity() > 0).map(Dump::Container))
    }

    fn get_request(&self) -> Option<Request> {
        self.spawn.resolve().filter(|spawn| spawn.free_energy_capacity() > 0).map(Request::Spawn)
            .or_else(|| 
                self.extensions.iter()
                    .filter_map(PlannedStructureRef::resolve)
                    .max_by_key(EnergyStoreAccessors::free_energy_capacity)
                    .filter(|extension| extension.free_energy_capacity() > 0)
                    .map(Request::Extension)
            )
    }
}

#[derive(Clone)]
enum Work {
    Build(ConstructionSite),
    Repair(RepairableStructure)
}

impl VirtualCreep {
    fn work(&mut self, work: Work) -> Flow {
        match work {
            Work::Build(site) => self.build(site),
            Work::Repair(structure) => self.repair(structure),
        }.map_continue(|_| ())
    }
}

#[derive(Clone)]
enum Dump {
    Container(StructureContainer),
    Link(StructureLink)
}

impl HasStore for Dump {
    fn store(&self) -> Store {
        match self {
            Dump::Container(container) => container.store(),
            Dump::Link(link) => link.store(),
        }
    }
}

impl Transferable for Dump {
    fn transfer_from(&self, creep: &screeps::Creep, ty: ResourceType, amount: Option<u32>) -> Result<(), screeps::action_error_codes::TransferErrorCode> {
        match self {
            Dump::Container(container) => container.transfer_from(creep, ty, amount),
            Dump::Link(link) => link.transfer_from(creep, ty, amount),
        }
    }
}

#[derive(Clone)]
enum Request {
    Spawn(StructureSpawn),
    Extension(StructureExtension)
}

impl HasStore for Request {
    fn store(&self) -> Store {
        match self {
            Request::Spawn(spawn) => spawn.store(),
            Request::Extension(extension) => extension.store(),
        }
    }
}

impl Transferable for Request {
    fn transfer_from(&self, creep: &screeps::Creep, ty: ResourceType, amount: Option<u32>) -> Result<(), screeps::action_error_codes::TransferErrorCode> {
        match self {
            Request::Spawn(spawn) => spawn.transfer_from(creep, ty, amount),
            Request::Extension(extension) => extension.transfer_from(creep, ty, amount),
        }
    }
}

pub fn is_in_position(creep_pos: Position, source: &ObjectId<Source>, home: &ColonyView) -> bool {
    home.plan.sources.get(&source.screeps_id()).is_some_and(|plan| plan.container.pos == creep_pos)
}

fn fill_up(creep: &mut VirtualCreep, container: Option<StructureContainer>, amount: u32) -> Flow {
    let container = container.or_break()?;

    let amount = amount.min(container.used_energy_capacity());
    break_if(amount == 0)?;

    creep.withdraw_energy(container, Some(amount))?;

    Flow::Continue(())
}

pub fn update(creep: &mut VirtualCreep, source: &Source, home: &ColonyView<'_>) -> Stop {
    let plan = home.plan.sources.get(&source.id()).ok_or(anyhow!("Source plan doesn't exist"))?;

    creep.approach_tugged(plan.container.pos, 0)?;

    let harvest_energy = (creep.body().part_count(Part::Work) * 2) as u32;

    let may_work = source.energy() <= source.ticks_to_regeneration().unwrap_or(ENERGY_REGEN_TIME).saturating_sub(1) * harvest_energy;
    let work = may_work.then(|| plan.get_work()).flatten();

    let request = plan.get_request();

    let harvest = source.energy() > 0;
    if harvest { creep.harvest_source(source.clone())?; }

    let headroom = if harvest { harvest_energy } else { 0 };

    let container = plan.container.resolve();
    let can_gain_energy = harvest || container.as_ref().is_some_and(|container| container.used_energy_capacity() > 0);

    let is_full = creep.next_free_capacity() < headroom.max(1);

    if (is_full || !can_gain_energy) && creep.curr_used_energy_capacity() > 0 {
        if let Some(request) = request {
            creep.transfer_energy(request, None)?;
        } else if let Some(work) = work {
            creep.cancel_intent(IntentType::Harvest);
            creep.work(work)?;
        } else {
            let dump = plan.get_dump().or_break()?;
            creep.transfer_energy(dump, None)?;
        }

        return Flow::Break
    }

    if request.is_some() {
        fill_up(creep, container, creep.next_free_capacity().saturating_sub(headroom))?;
    } else if work.is_some() {
        fill_up(creep, container, creep.next_free_capacity())?;
    }

    Flow::Break
}