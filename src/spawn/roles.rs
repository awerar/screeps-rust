use screeps::{Part, Source};

use crate::{creeps::CreepRole, domain_traits::{CreepId, ObjectId}, spawn::{prototype::{AbsolutePrototype, RelativePrototype}, roster::{ColonyCreeps, GlobalCreeps}}};

pub enum RoleSelector {
    SourceExcavator(ObjectId<Source>),
    Truck,
    Flagship,
    TugboatFor(CreepId),
    Fabricator,
    Servicer,
    Upgrader,
    Seeder,
    Terraformer
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
            RoleSelector::Upgrader => matches!(role, CreepRole::Terraformer | CreepRole::Seeder),
            RoleSelector::Seeder => matches!(role, CreepRole::Seeder),
            RoleSelector::Terraformer => matches!(role, CreepRole::Terraformer)
        }
    }
}

impl ColonyCreeps {
    pub fn of_role(&self, role: RoleSelector) -> impl Iterator<Item = (&CreepId, &RelativePrototype)> {
        self.iter().filter(move |(_, proto)| role.matches(proto.role()))
    }

    pub fn any_of_role(&self, role: RoleSelector) -> bool {
        self.iter().any(move |(_, proto)| role.matches(proto.role()))
    }

    pub fn part_count(&self, role: RoleSelector, part: Part) -> usize {
        self.of_role(role).map(|(_, proto)| proto.body().part_count(part)).sum()
    }
}

impl GlobalCreeps {
    pub fn of_role(&self, role: RoleSelector) -> impl Iterator<Item = (&CreepId, &AbsolutePrototype)> {
        self.iter().filter(move |(_, proto)| role.matches(proto.role()))
    }

    #[expect(unused)]
    pub fn part_count(&self, role: RoleSelector, part: Part) -> usize {
        self.of_role(role).map(|(_, proto)| proto.body().part_count(part)).sum()
    }
}
