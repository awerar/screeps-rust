use screeps::{BUILD_POWER, HARVEST_POWER, Part, RoomName, UPGRADE_CONTROLLER_POWER};

use crate::{colony::ColonyView, creeps::CreepRole, domain_traits::EnergyStoreAccessors, spawn::{prototype::{AbsolutePrototype, RelativePrototype}, roster::GlobalCreeps}};

#[derive(Clone)]
pub struct ColonyBudget {
    colony: RoomName,
    creep_budget: f32,
    storage_budget: f32
}

impl ColonyBudget {
    pub fn new(colony: &ColonyView<'_>, creeps: &GlobalCreeps) -> Self {
        let mut budget = Self { 
            colony: colony.name, 
            creep_budget: 0.0, 
            storage_budget: colony.buffer.map_or_default(|buffer| buffer.used_energy_capacity() as f32 / 1500 as f32)
        };

        for (_, proto) in creeps.iter() {
            budget.add_creep(proto);
        }

        budget
    }

    fn add_local_creep(&mut self, proto: &RelativePrototype) {
        // TODO: Make more accurate
        let work_count = proto.body().part_count(Part::Work) as f32;
        let role_contribution = match proto.role() {
            CreepRole::Excavator(_) => (work_count * HARVEST_POWER as f32).min(10.0),
            CreepRole::Fabricator(_) => -work_count * BUILD_POWER as f32,
            CreepRole::Servicer(_) => -work_count,
            CreepRole::Terraformer(_) | CreepRole::Seeder(_) => -work_count * UPGRADE_CONTROLLER_POWER as f32,
            CreepRole::ExportTruck(truck) => -truck.power(proto.body()),
            CreepRole::Flagship(_) | CreepRole::Truck(_) 
            | CreepRole::Tugboat(_) 
            | CreepRole::Scrap(_) => 0.0,
        };

        self.creep_budget += role_contribution;
    } 

    fn add_nonlocal_creep(&mut self, proto: &AbsolutePrototype) {
        if let CreepRole::ExportTruck(truck) = proto.role() {
            if truck.import_colony() == self.colony {
                self.creep_budget += truck.power(proto.body());
            }
        }
    }

    pub fn add_creep(&mut self, proto: &AbsolutePrototype) {
        if proto.spawn_room() == self.colony { 
            self.creep_budget -= proto.body().energy_drain();
        }

        if proto.home() == self.colony {
            self.add_local_creep(proto.relative());
        } else {
            self.add_nonlocal_creep(proto);
        }
    }

    pub fn full(&self) -> f32 {
        self.creep_budget + self.storage_budget
    }

    pub fn sustainable(&self) -> f32 {
        self.creep_budget
    }
}