use screeps::{Creep, HasPosition};
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
        use HarvesterState::*;

        let Some(CreepData { role: CreepRole::Harvester(_, source), .. }) = mem.creep(creep) else { return Err(()) };
        let source = source.resolve().ok_or(())?;

        match self.clone() {
            Going(mut tugged_state) => {
                tugged_state.move_tugged_to(creep, mem, source.pos(), 1);
                if tugged_state.is_finished() {
                    Ok(Mining)
                } else {
                    Ok(Going(tugged_state))
                }
            },
            Mining => {
                Ok(Mining)
            },
        }
    }
}