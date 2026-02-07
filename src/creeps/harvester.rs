use screeps::{ObjectId, Position, Source};
use serde::{Deserialize, Serialize};

use crate::creeps::CreepState;

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Default, Clone)]
pub enum HarvesterState {
    #[default]
    Idle,
    Going,
    Mining
}

impl CreepState for HarvesterState {
    fn update(&self, creep: &screeps::Creep, mem: &mut crate::memory::Memory) -> Result<Self, ()> {
        todo!()
    }
}