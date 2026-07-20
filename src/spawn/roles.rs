use screeps::{Part, Source};

use crate::{creeps::CreepRole, domain_traits::{CreepId, ObjectId}, spawn::{prototype::{AbsolutePrototype, RelativePrototype}, roster::{ColonyCreeps, GlobalCreeps}}};

pub enum RoleSelector {
    Excavator,
    SourceExcavator(ObjectId<Source>),
    Truck,
    ImportTruck,
    Flagship,
    Tugboat,
    TugboatFor(CreepId),
    Fabricator
}

impl RoleSelector {
    fn matches(&self, role: &CreepRole) -> bool {
        match self {
            RoleSelector::Excavator => matches!(role, CreepRole::Excavator(_, _)),
            RoleSelector::SourceExcavator(source) => matches!(role, CreepRole::Excavator(_, source2) if *source2 == *source),
            RoleSelector::Truck => matches!(role, CreepRole::Truck(_)),
            RoleSelector::ImportTruck => matches!(role, CreepRole::ImportTruck(_)),
            RoleSelector::Flagship => matches!(role, CreepRole::Flagship(_)),
            RoleSelector::Tugboat => matches!(role, CreepRole::Tugboat(_, _)),
            RoleSelector::TugboatFor(tugged) => matches!(role, CreepRole::Tugboat(tugged2, _) if *tugged2 == *tugged),
            RoleSelector::Fabricator => matches!(role, CreepRole::Fabricator(_)),
        }
    }
}

impl ColonyCreeps {
    pub fn of_role(&self, role: RoleSelector) -> impl Iterator<Item = &RelativePrototype> {
        self.values().filter(move |proto| role.matches(proto.role()))
    }

    pub fn part_count(&self, role: RoleSelector, part: Part) -> usize {
        self.of_role(role).map(|proto| proto.body().part_count(part)).sum()
    }
}

impl GlobalCreeps {
    pub fn of_role(&self, role: RoleSelector) -> impl Iterator<Item = &AbsolutePrototype> {
        self.values().filter(move |proto| role.matches(proto.role()))
    }

    pub fn part_count(&self, role: RoleSelector, part: Part) -> usize {
        self.of_role(role).map(|proto| proto.body().part_count(part)).sum()
    }
}
