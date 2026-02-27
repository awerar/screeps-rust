use std::collections::{HashMap, HashSet, VecDeque};

use js_sys::{JsString, Reflect};
use log::{error, warn};
use screeps::{Creep, Position, RoomName};

use serde::{Deserialize, Serialize};

use crate::{callbacks::Callbacks, checked_id::{CheckIDs, CheckedID, TryCheckIDs}, colony::Colonies, creeps::{CreepData, fabricator::FabricatorCoordinator, truck::TruckCoordinator}, messages::Messages, movement::Movement, statemachine::UnderlyingName};

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
    
    #[serde(default)] pub tick_times: VecDeque<f64>,

    #[serde(rename = "internal_creeps")]
    #[serde(default)] pub creeps: HashMap<CheckedID<Creep>, CreepData>,
    #[serde(default)] pub colonies: Colonies,

    #[serde(default)] pub incoming_creeps: Vec<(String, CreepData)>,
    #[serde(default)] pub callbacks: Callbacks,
    #[serde(default)] pub movement: Movement,
    #[serde(default)] pub claim_requests: ClaimRequests,
    #[serde(default)] pub truck_coordinators: HashMap<RoomName, TruckCoordinator>,
    #[serde(default)] pub fabricator_coordinators: HashMap<RoomName, FabricatorCoordinator>,

    #[serde(default)] pub messages: Messages
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

        match serde_json::to_string(&self) {
            Ok(mem) => screeps::raw_memory::set(&JsString::from(mem)),
            Err(e) => error!("Unable to serialize memory: {e}"),
        }
    }

    pub fn get_average_tick_rate_over(&self, tick_count: usize) -> f64 {
        self.tick_times.iter().take(tick_count).sum::<f64>() / (tick_count.min(self.tick_times.len()) as f64)
    }
}

impl CheckIDs for Memory {
    fn check_ids(mut self) -> Self {
        self.movement = self.movement.check_ids();
        self.messages = self.messages.check_ids();
        self.creeps = self.creeps.into_iter()
            .filter_map(|(creep_id, creep_data)| Some((creep_id.try_check_ids()?, creep_data.try_check_ids()?)))
            .collect();

        self
    }
}