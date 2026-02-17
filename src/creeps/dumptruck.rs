use screeps::Creep;
use serde::{Deserialize, Serialize};

use crate::{memory::Memory, statemachine::StateMachine};

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Default)]
pub enum DumptruckCreep {
    #[default] Idle
}

impl StateMachine<Creep> for DumptruckCreep {
    fn update(&self, _creep: &Creep, _mem: &mut Memory) -> Result<Self, ()> {
        todo!()
    }
}