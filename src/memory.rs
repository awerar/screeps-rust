use std::collections::{HashMap, HashSet, VecDeque};

use js_sys::JsString;
use log::warn;
use screeps::{Position, RoomName};

use serde::{Deserialize, Serialize};

use crate::{callbacks::Callbacks, check::deserialize_filter_check, colony::Colonies, creeps::{CreepData, Creeps, fabricator::FabricatorCoordinator, truck::TruckCoordinator}, movement::MovementMemory};

extern crate serde_json_path_to_error as serde_json;

#[derive(Serialize, Deserialize, Default)]
pub struct Memory {
    #[serde(rename = "allies")]
    _alliance_allies: Option<serde_json::Value>,
    #[serde(rename = "myData")]
    _alliance_my_data: Option<serde_json::Value>,
    #[serde(rename = "alliesData")]
    _alliance_allies_data: Option<serde_json::Value>,
    
    pub tick_times: VecDeque<f64>,

    pub creeps: Creeps,
    pub colonies: Colonies,

    #[serde(deserialize_with = "deserialize_filter_check")]
    pub incoming_creeps: Vec<(String, CreepData)>,
    pub callbacks: Callbacks,
    #[serde(deserialize_with = "deserialize_filter_check")]
    pub claim_requests: ClaimRequests,
    pub truck_coordinators: HashMap<RoomName, TruckCoordinator>,
    pub fabricator_coordinators: HashMap<RoomName, FabricatorCoordinator>,
    pub movement: MovementMemory
}

pub type ClaimRequests = HashSet<Position>;

impl Memory {
    pub fn screeps_deserialize() -> Self {
        serde_json::from_str(&String::from(screeps::raw_memory::get())).unwrap_or_else(|_| {
            warn!("Unable to parse raw memory. Resetting memory");
            Memory::default()
        })
    }

    pub fn screeps_serialize(self) {
        screeps::raw_memory::set(&JsString::from(serde_json::to_string(&self).unwrap()));
    }

    pub fn get_average_tick_rate_over(&self, tick_count: usize) -> f64 {
        self.tick_times.iter().take(tick_count).sum::<f64>() / (tick_count.min(self.tick_times.len()) as f64)
    }
}