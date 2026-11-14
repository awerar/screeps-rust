use std::collections::HashMap;

use derive_deref::Deref;

use crate::{colony::ColonyName, domain_traits::CreepId, memory::Memory, spawn::scheduler::prototype::{AbsolutePrototype, RelativePrototype}};

#[derive(Deref)]
pub struct KnownCreeps(HashMap<CreepId, AbsolutePrototype>);

impl KnownCreeps {
    pub fn new(mem: &Memory) -> Self {
        Self(mem.creeps.iter()
            .map(|(id, data)| (id.clone(), AbsolutePrototype::from_creep(id, data)))
            .collect())
    }

    pub fn of_home(&self, home: ColonyName) -> HomeCreeps<'_> {
        HomeCreeps { creeps: self, home }
    }

    pub fn insert(&mut self, id: CreepId, proto: AbsolutePrototype) {
        self.0.insert(id, proto);
    }
}

#[derive(Clone, Copy)]
pub struct HomeCreeps<'a> {
    creeps: &'a KnownCreeps,
    home: ColonyName
}

impl<'a> HomeCreeps<'a> {
    pub fn iter(self) -> impl Iterator<Item = (&'a CreepId, &'a RelativePrototype)> {
        self.creeps.iter()
            .filter(move |(_, proto)| proto.home() == self.home)
            .map(|(id, proto)| (id, proto.relative()))
    }

    pub fn contains(self, id: &CreepId) -> bool {
        self.creeps.get(id).is_some_and(|proto| proto.home() == self.home)
    }
}
