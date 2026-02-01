use std::{collections::{HashMap, HashSet}, sync::LazyLock};

use itertools::Itertools;
use log::*;
use screeps::{
    ConstructionSite, Position, ResourceType, RoomName, StructureController, StructureExtension, StructureObject, StructureSpawn, StructureStorage, StructureTower, StructureType, find, game, local::ObjectId, objects::{Creep, Source}, prelude::*
};
use serde::{Deserialize, Serialize};

use crate::{creeps::{CreepData, transition}, memory::SharedMemory};

extern crate serde_json_path_to_error as serde_json;

#[derive(Serialize, Deserialize, Debug)]
pub struct BootstrapCarrier {
    home: RoomName,
    state: BootstrapCarrierState
}

#[derive(Serialize, Deserialize, Debug)]
enum BootstrapCarrierState {
    Idle,
    Refilling,
    Bootstrapping(Position)
}

impl Default for BootstrapCarrierState {
    fn default() -> Self {
        BootstrapCarrierState::Idle
    }
}

impl BootstrapCarrierState {
    fn execute(self, creep: &Creep, memory: &mut SharedMemory, home: &RoomName) -> Option<Self> {
        todo!()
    }
}

impl CreepData for BootstrapCarrier {
    fn perform(&mut self, creep: &Creep, memory: &mut SharedMemory) {
        transition(&mut self.state, creep, memory, 
            |state, creep, memory| state.execute(creep, memory, &self.home)
        );
    }
}