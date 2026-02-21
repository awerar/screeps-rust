use screeps::Creep;
use serde::{Serialize, Deserialize};

use crate::{memory::Memory, statemachine::{StateMachine, Transition}};

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Default)]
pub enum FabricatorCreep {
    #[default] Idle
}

impl StateMachine<Creep> for FabricatorCreep {
    fn update(&self, creep: &Creep, mem: &mut Memory) -> Result<Transition<Self>, ()> {
        todo!()
    }
}