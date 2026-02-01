use std::{collections::{HashMap, HashSet}, sync::LazyLock};

use itertools::Itertools;
use log::*;
use screeps::{
    ConstructionSite, Position, ResourceType, RoomName, StructureController, StructureExtension, StructureObject, StructureSpawn, StructureStorage, StructureTower, StructureType, find, game, local::ObjectId, objects::{Creep, Source}, prelude::*
};
use serde::{Deserialize, Serialize};

use crate::{creeps::CreepState, memory::SharedMemory};

extern crate serde_json_path_to_error as serde_json;

#[derive(Serialize, Deserialize, Debug)]
pub enum BootstrapCarrierState {
    Idle,
    Refilling,
    Bootstrapping(Position)
}

impl Default for BootstrapCarrierState {
    fn default() -> Self {
        BootstrapCarrierState::Idle
    }
}

impl CreepState<RoomName> for BootstrapCarrierState {
    fn execute(self, home: &mut RoomName, creep: &Creep, memory: &mut SharedMemory) -> Option<Self> {
        todo!()
    }
}