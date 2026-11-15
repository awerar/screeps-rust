use screeps::{CREEP_LIFE_TIME, Part, Source};

use crate::{creeps::CreepRole, domain_traits::{CreepId, ObjectId, ResolvableId}, spawn::scheduler::{creeps::{KnownCreeps, HomeCreeps}, prototype::{AbsolutePrototype, RelativePrototype}}};

pub enum RoleSelector {
    SourceExcavator(ObjectId<Source>),
    Truck,
    Flagship,
    TugboatFor(CreepId),
    Fabricator,
    Servicer,
    Terraformer,
    Upgrader
}

impl RoleSelector {
    fn matches(&self, role: &CreepRole) -> bool {
        match self {
            RoleSelector::SourceExcavator(source) => matches!(role, CreepRole::Excavator(source2) if source == source2),
            RoleSelector::Truck => matches!(role, CreepRole::Truck(_)),
            RoleSelector::Flagship => matches!(role, CreepRole::Flagship(_)),
            RoleSelector::TugboatFor(tugged) => matches!(role, CreepRole::Tugboat(tugged2) if *tugged2 == *tugged),
            RoleSelector::Fabricator => matches!(role, CreepRole::Fabricator(_)),
            RoleSelector::Servicer => matches!(role, CreepRole::Servicer(_)),
            RoleSelector::Terraformer => matches!(role, CreepRole::Terraformer),
            RoleSelector::Upgrader => matches!(role, CreepRole::Terraformer | CreepRole::Seeder)
        }
    }
}

impl<'a> HomeCreeps<'a> {
    pub fn of_role(self, role: RoleSelector) -> impl Iterator<Item = (&'a CreepId, &'a RelativePrototype)> {
        self.iter().filter(move |(_, proto)| role.matches(proto.role()))
    }

    pub fn any_of_role(self, role: RoleSelector) -> bool {
        self.of_role(role).next().is_some()
    }

    pub fn part_count(self, role: RoleSelector, part: Part) -> usize {
        self.of_role(role).map(|(_, proto)| proto.body().part_count(part)).sum()
    }

    pub fn part_ticks_left_of_role(self, role: RoleSelector, part: Part) -> u32 {
        self.of_role(role)
            .map(|(creep , proto)| {
                creep.resolve().ticks_to_live().unwrap_or(CREEP_LIFE_TIME) * proto.body().part_count(part) as u32
            })
            .sum()
    }
}

impl KnownCreeps {
    pub fn of_role(&self, role: RoleSelector) -> impl Iterator<Item = (&CreepId, &AbsolutePrototype)> {
        self.iter().filter(move |(_, proto)| role.matches(proto.role()))
    }

    pub fn any_of_role(&self, role: RoleSelector) -> bool {
        self.of_role(role).next().is_some()
    }
}
