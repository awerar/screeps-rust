use std::{collections::HashMap};

use screeps::{BUILD_POWER, HARVEST_POWER, LINK_LOSS_RATIO, ObjectId, Part, Source, UPGRADE_CONTROLLER_POWER};

use crate::{colony::{ColonyName, ColonyView}, creeps::{CreepRole, body::Body, fabricator::FabricatorCreep, servicer::ServicerCreep}, domain_traits::EnergyStoreAccessors, spawn::{policies::BASELINE_UPGRADE_WORKS, scheduler::{creeps::KnownCreeps, prototype::{AbsolutePrototype, RelativePrototype}}}};

const SOURCE_POWER: u32 = 10;
const HARVEST_WORKS: u32 = SOURCE_POWER.div_exact(HARVEST_POWER).unwrap();

#[derive(Clone)]
struct SourceState {
    has_work: bool,
    has_link: bool
}

#[derive(Clone)]
pub struct ColonyBudget {
    colony: ColonyName,

    income: f32,
    general_drain: f32,
    upgrade_drain: f32,

    storage_budget: f32,

    sources: HashMap<ObjectId<Source>, SourceState>
}

impl ColonyBudget {
    pub fn new(colony: &ColonyView<'_>, creeps: &KnownCreeps) -> Self {
        let sources = colony.plan.sources.iter()
            .map(|(id, source_plan)| (*id, SourceState {
                has_work: source_plan.has_work(),
                has_link: source_plan.link.is_complete()
            }))
            .collect();

        let mut budget = Self {
            colony: colony.name,
            income: 0.0,
            general_drain: 0.0,
            upgrade_drain: 0.0,
            storage_budget: colony.buffer.map_or_default(|buffer| buffer.used_energy_capacity() as f32 / 1500.0),
            sources
        };

        for proto in creeps.values() {
            budget.add_creep(proto);
        }

        budget
    }

    fn split_terraformer_works(works: u32) -> (u32, u32) {
        let baseline_work = works.min(BASELINE_UPGRADE_WORKS);
        let extra_work = works - baseline_work;

        (baseline_work, extra_work)
    }

    fn add_local_creep_contribution(&mut self, proto: &RelativePrototype) {
        let works = proto.body().part_count(Part::Work) as u32;

        if proto.spawn_room() == self.colony.room_name() {
            match proto.role() {
                CreepRole::Seeder => self.upgrade_drain += proto.body().energy_drain(),
                CreepRole::Terraformer => {
                    let (baseline_works, extra_works) = Self::split_terraformer_works(works);

                    let base_body = Body::of_part(Part::Carry, 1) + Body::of_part(Part::Work, baseline_works as usize);
                    let extra_body = Body::of_part(Part::Work, extra_works as usize);

                    assert!(proto.body().has_same_composition_as(&(base_body.clone() + extra_body.clone())));

                    self.general_drain += base_body.energy_drain();
                    self.upgrade_drain += extra_body.energy_drain();
                },
                _ => self.general_drain += proto.body().energy_drain()
            }
        }

        match proto.role() {
            CreepRole::Fabricator(FabricatorCreep::Scrap(_))
            | CreepRole::Servicer(ServicerCreep::Scrap(_)) 
            | CreepRole::Flagship(_) | CreepRole::Truck(_)
            | CreepRole::Tugboat(_) | CreepRole::Scrap(_) => (),
            CreepRole::Excavator(source) => {
                let source = &self.sources[&source.screeps_id()];
                
                let harvest_works = works.min(HARVEST_WORKS);
                let build_drain = (if source.has_work { (works - harvest_works) * BUILD_POWER } else { 0 }) as f32;
                
                let mut income = (harvest_works * HARVEST_POWER) as f32;

                if source.has_link {
                    income = build_drain + (income - build_drain) * (1.0 - LINK_LOSS_RATIO);
                }

                self.income += income;
                self.general_drain += build_drain;
            },
            CreepRole::Fabricator(_) => self.general_drain += (works * BUILD_POWER) as f32,
            CreepRole::Servicer(_) => self.general_drain += works as f32,
            CreepRole::Terraformer => {
                let (baseline_work, extra_work) = Self::split_terraformer_works(works);

                self.general_drain += (baseline_work * UPGRADE_CONTROLLER_POWER) as f32;
                self.upgrade_drain += (extra_work * UPGRADE_CONTROLLER_POWER) as f32;
            },
            CreepRole::Seeder => self.upgrade_drain += (works * UPGRADE_CONTROLLER_POWER) as f32,
            CreepRole::ExportTruck(truck) => self.general_drain += truck.power(proto.body()),
        }
    }

    fn add_nonlocal_creep_contribution(&mut self, proto: &AbsolutePrototype) {
        if let CreepRole::ExportTruck(truck) = proto.role() && truck.import_colony() == self.colony {
            self.income += truck.power(proto.body());
        }

        if proto.spawn_room() == self.colony.room_name() { // TODO: Add also in other branch
            self.general_drain += proto.body().energy_drain();
        }
    }

    pub fn add_creep(&mut self, proto: &AbsolutePrototype) {
        if proto.home() == self.colony {
            self.add_local_creep_contribution(proto.relative())
        } else {
            self.add_nonlocal_creep_contribution(proto)
        }
    }

    pub fn general(&self) -> f32 {
        self.income + self.storage_budget - self.general_drain
    }

    // How much energy we can spend above baseline on upgrades each tick
    pub fn upgrade(&self) -> f32 {
        self.income - self.general_drain
    }

    // How much energy we can spend on seeders each tick
    pub fn seeder(&self) -> f32 {
        self.income - self.general_drain - self.upgrade_drain
    }

    pub fn income(&self) -> f32 {
        self.income
    }
}