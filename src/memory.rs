use std::collections::{HashMap, HashSet, VecDeque};

use js_sys::{JsString, Reflect};
use log::warn;
use screeps::{Position, RoomName};

use serde::{Deserialize, Deserializer, Serialize};

use crate::{callbacks::Callbacks, colony::Colonies, creeps::{CreepData, Creeps, fabricator::FabricatorCoordinator, truck::TruckCoordinator}, messages::Messages, movement::Movement, safeid::{TryMakeSafe, UnsafeIDs}};

extern crate serde_json_path_to_error as serde_json;

#[derive(Serialize, Deserialize, Default)]
pub struct Memory {
    #[serde(rename = "creeps")]
    _internal_creeps: Option<serde_json::Value>,

    #[serde(rename = "allies")]
    _alliance_allies: Option<serde_json::Value>,
    #[serde(rename = "myData")]
    _alliance_my_data: Option<serde_json::Value>,
    #[serde(rename = "alliesData")]
    _alliance_allies_data: Option<serde_json::Value>,
    
    pub tick_times: VecDeque<f64>,

    #[serde(rename = "internal_creeps")]
    pub creeps: Creeps,
    pub colonies: Colonies,

    #[serde(deserialize_with = "deserialize_prune_incoming_creeps")]
    pub incoming_creeps: Vec<(String, CreepData)>,
    pub callbacks: Callbacks,
    pub movement: Movement,
    pub claim_requests: ClaimRequests,
    pub truck_coordinators: HashMap<RoomName, TruckCoordinator>,
    pub fabricator_coordinators: HashMap<RoomName, FabricatorCoordinator>,

    pub messages: Messages
}

pub type ClaimRequests = HashSet<Position>;

impl Memory {
    #[expect(clippy::used_underscore_binding)]
    pub fn screeps_deserialize() -> Self {
        let mem = screeps::raw_memory::get();
        let mut mem: Memory = serde_json::from_str(&String::from(mem)).unwrap_or_else(|_| {
            warn!("Unable to parse raw memory. Resetting memory");
            Memory::default()
        });

        mem._internal_creeps = None; // This is deserialized separately in JS
        mem
    }

    #[expect(clippy::used_underscore_binding)]
    pub fn screeps_serialize(&mut self) {
        #[allow(deprecated)]
        let new_internal_creeps = Reflect::get(&screeps::memory::ROOT, &JsString::from("creeps")).ok();
        let new_internal_creeps: Option<serde_json::Value> = new_internal_creeps.map(|x| serde_wasm_bindgen::from_value(x).unwrap());
        self._internal_creeps = new_internal_creeps;

        let mem = serde_json::to_string(&self).unwrap();
        screeps::raw_memory::set(&JsString::from(mem));
    }

    pub fn get_average_tick_rate_over(&self, tick_count: usize) -> f64 {
        self.tick_times.iter().take(tick_count).sum::<f64>() / (tick_count.min(self.tick_times.len()) as f64)
    }
}

fn deserialize_prune_incoming_creeps<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<(String, CreepData)>, D::Error> {
    let raw = Vec::<(String, CreepData<UnsafeIDs>)>::deserialize(deserializer)?;
    Ok(raw.into_iter().filter_map(|(k, v)| Some((k, v.try_make_safe()?))).collect())
}