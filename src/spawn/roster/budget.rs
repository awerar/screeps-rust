use std::{collections::HashMap, ops::{Add, AddAssign}};

use screeps::{BUILD_POWER, HARVEST_POWER, LINK_LOSS_RATIO, ObjectId, Part, RoomName, Source, UPGRADE_CONTROLLER_POWER};

use crate::{colony::ColonyView, creeps::CreepRole, domain_traits::EnergyStoreAccessors, spawn::roster::{creeps::GlobalCreeps, prototype::{AbsolutePrototype, RelativePrototype}}};

const SOURCE_POWER: f32 = 10.0;

#[derive(Clone)]
struct SourceState {
    has_work: bool,
    has_link: bool
}

fn excavator_contribution(work_count: f32, state: &SourceState) -> CreepBudget {
    let harvest_works = work_count.min(SOURCE_POWER / HARVEST_POWER as f32);
    let mut income = harvest_works * HARVEST_POWER as f32;

    let build_drain = if state.has_work { (work_count - harvest_works) * BUILD_POWER as f32 } else { 0.0 };

    if state.has_link {
        income = build_drain + (income - build_drain) * (1.0 - LINK_LOSS_RATIO);
    }

    CreepBudget { drain: build_drain, income }
}

#[derive(Default, Clone, Copy)]
struct CreepBudget {
    drain: f32,
    income: f32
}

impl CreepBudget {
    fn drain(drain: f32) -> Self {
        CreepBudget { drain, income: 0.0 }
    }

    fn income(income: f32) -> Self {
        CreepBudget { drain: 0.0, income }
    }

    pub fn net(self) -> f32 {
        self.income - self.drain
    }
}

impl Add for CreepBudget {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self { drain: self.drain + rhs.drain, income: self.income + rhs.income }
    }
}

impl AddAssign for CreepBudget {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

#[derive(Clone)]
pub struct ColonyBudget {
    colony: RoomName,

    creep_budget: CreepBudget,
    storage_budget: f32,

    sources: HashMap<ObjectId<Source>, SourceState>
}

impl ColonyBudget {
    pub fn new(colony: &ColonyView<'_>, creeps: &GlobalCreeps) -> Self {
        let sources = colony.plan.sources.iter()
            .map(|(id, source_plan)| (*id, SourceState {
                has_work: source_plan.has_work(),
                has_link: source_plan.link.is_complete()
            }))
            .collect();

        let mut budget = Self {
            colony: colony.name,
            creep_budget: CreepBudget::default(),
            storage_budget: colony.buffer.map_or_default(|buffer| buffer.used_energy_capacity() as f32 / 1500.0),
            sources
        };

        for proto in creeps.values() {
            budget.add_creep(proto);
        }

        budget
    }

    fn get_local_creep_contribution(&self, proto: &RelativePrototype) -> CreepBudget {
        let work_count = proto.body().part_count(Part::Work) as f32;
        match proto.role() {
            CreepRole::Excavator(source) => excavator_contribution(work_count, &self.sources[&source.screeps_id()]),
            CreepRole::Fabricator(_) => CreepBudget::drain(work_count * BUILD_POWER as f32),
            CreepRole::Servicer(_) => CreepBudget::drain(work_count),
            CreepRole::Terraformer | CreepRole::Seeder => CreepBudget::drain(work_count * UPGRADE_CONTROLLER_POWER as f32),
            CreepRole::ExportTruck(truck) => CreepBudget::drain(truck.power(proto.body())),
            CreepRole::Flagship(_) | CreepRole::Truck(_)
            | CreepRole::Tugboat(_) | CreepRole::Scrap(_) => CreepBudget::default(),
        }
    }

    fn get_nonlocal_creep_contribution(&self, proto: &AbsolutePrototype) -> CreepBudget {
        if let CreepRole::ExportTruck(truck) = proto.role() && truck.import_colony() == self.colony {
            CreepBudget::income(truck.power(proto.body()))
        } else {
            CreepBudget::default()
        }
    }

    fn get_creep_contribution(&self, proto: &AbsolutePrototype) -> CreepBudget {
        let mut contribution = CreepBudget::default();

        if proto.spawn_room() == self.colony {
            contribution += CreepBudget::drain(proto.body().energy_drain());
        }

        if proto.home() == self.colony {
            contribution += self.get_local_creep_contribution(proto.relative());
        } else {
            contribution += self.get_nonlocal_creep_contribution(proto);
        }

        contribution
    }

    pub fn add_creep(&mut self, proto: &AbsolutePrototype) {
        self.creep_budget += self.get_creep_contribution(proto);
    }

    pub fn full(&self) -> f32 {
        self.sustainable() + self.storage_budget
    }

    pub fn sustainable(&self) -> f32 {
        self.creep_budget.net()
    }

    pub fn income(&self) -> f32 {
        self.creep_budget.income
    }
}