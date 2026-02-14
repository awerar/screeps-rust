use screeps::{Creep, HasPosition};
use serde::{Deserialize, Serialize};

use crate::{creeps::{CreepData, CreepRole, tugboat::TuggedCreep}, memory::Memory, statemachine::StateMachine};

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum HarvesterCreep {
    Going(TuggedCreep),
    Mining
}

impl Default for HarvesterCreep {
    fn default() -> Self {
        Self::Going(Default::default())
    }
}

impl StateMachine<Creep> for HarvesterCreep {
    fn update(&self, creep: &Creep, mem: &mut Memory) -> Result<Self, ()> {
        use HarvesterCreep::*;

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