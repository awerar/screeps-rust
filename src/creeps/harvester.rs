use screeps::Creep;
use serde::{Deserialize, Serialize};

use crate::{creeps::{CreepData, CreepRole, CreepState, tugboat::TuggedState}, memory::Memory};

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum HarvesterState {
    Going(TuggedState),
    Mining
}

impl Default for HarvesterState {
    fn default() -> Self {
        Self::Going(Default::default())
    }
}

impl CreepState for HarvesterState {
    fn update(&self, creep: &Creep, mem: &mut Memory) -> Result<Self, ()> {
        let Some(CreepData { home, role: CreepRole::Harvester(_, source) }) = mem.creep(creep) else { return Err(()) };

        match self {
            HarvesterState::Going(tugged_state) => todo!(),
            HarvesterState::Mining => todo!(),
        }
    }
}