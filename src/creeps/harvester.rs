use screeps::Creep;
use serde::{Deserialize, Serialize};

use crate::{creeps::CreepState, memory::Memory};

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Default, Clone)]
pub enum HarvesterState {
    #[default]
    Idle,
    Going,
    Mining
}

impl CreepState for HarvesterState {
    fn update(&self, creep: &Creep, mem: &mut Memory) -> Result<Self, ()> {
        todo!()
    }
}