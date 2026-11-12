use std::collections::HashMap;

use screeps::RoomName;

use derive_deref::Deref;

use crate::{domain_traits::CreepId, memory::Memory, spawn::roster::prototype::{AbsolutePrototype, RelativePrototype}};

#[derive(Deref)]
pub struct ColonyCreeps(pub HashMap<CreepId, RelativePrototype>);

impl ColonyCreeps {
    pub fn new(colony: RoomName, mem: &Memory) -> Self {
        Self(
            mem.creeps.iter()
                .filter(|(_, data)| data.home == colony)
                .map(|(id, data)| (id.clone(), RelativePrototype::from_creep(id, data)))
                .collect()
        )
    }
}

#[derive(Deref)]
pub struct GlobalCreeps(HashMap<CreepId, AbsolutePrototype>);

impl GlobalCreeps {
    pub fn new(mem: &Memory) -> Self {
        Self(mem.creeps.iter()
            .map(|(id, data)| (id.clone(), AbsolutePrototype::from_creep(id, data)))
            .collect())
    }
}
