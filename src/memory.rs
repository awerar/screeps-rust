use std::{cell::RefCell, collections::{HashMap, HashSet, VecDeque}};

use js_sys::{JsString, Reflect};
use log::*;
use screeps::{Creep, Position, RoomName, SharedCreepProperties, game};

use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

use crate::{callbacks::Callbacks, colony::{ColonyConfig, ColonyState}, creeps::{CreepConfig, CreepRole, harvester::SourceAssignments}, movement::Movement, remote_build::RemoteBuildRequests};

extern crate serde_json_path_to_error as serde_json;

#[derive(Serialize, Deserialize)]
pub struct Memory {
    #[serde(rename = "creeps")]
    _internal_creeps: Option<serde_json::Value>,
    
    #[serde(default)] pub tick_times: VecDeque<f64>,

    #[serde(default)] pub machines: StateMachines,

    #[serde(rename = "creep_configs")]
    #[serde(default)] pub creeps: HashMap<String, CreepConfig>,
    #[serde(default)] pub colonies: HashMap<RoomName, ColonyConfig>,

    #[serde(default)] pub last_alive_creeps: HashSet<String>,
    #[serde(default)] pub source_assignments: HashMap<RoomName, SourceAssignments>,
    #[serde(default)] pub callbacks: Callbacks,
    #[serde(default)] pub movement: Movement,
    #[serde(default)] pub claim_requests: HashSet<Position>,
    #[serde(default)] pub remote_build_requests: RemoteBuildRequests
}

#[derive(Serialize, Deserialize, Default)]
pub struct StateMachines {
    #[serde(default)] pub creeps: HashMap<String, CreepRole>,
    #[serde(default)] pub colonies: HashMap<RoomName, ColonyState>,
}

thread_local! {
    static RESET_MEMORY: RefCell<bool> = RefCell::new(false);
    static RESET_TILE_USAGE: RefCell<bool> = RefCell::new(false);
    static RESET_SOURCE_ASSIGNMENTS: RefCell<bool> = RefCell::new(false);
}

#[wasm_bindgen]
pub fn reset_memory() {
    RESET_MEMORY.replace(true);
}

#[wasm_bindgen]
pub fn reset_source_assignments() {
    RESET_SOURCE_ASSIGNMENTS.replace(true);
}

impl Memory {
    pub fn creep(&self, creep: &Creep) -> Option<&CreepConfig> {
        self.creeps.get(&creep.name())
    }

    pub fn colony(&self, name: RoomName) -> Option<&ColonyConfig> {
        self.colonies.get(&name)
    }

    pub fn screeps_deserialize() -> Self {
        RESET_MEMORY.with_borrow_mut(|reset| {
            if *reset {
                screeps::raw_memory::set(&JsString::from("{}"));
                *reset = false;

                info!("Reset memory by command!");
            }
        });

        let mem = screeps::raw_memory::get();
        let mut mem: Memory = serde_json::from_str(&String::from(mem)).expect("Memory should follow correct schema");
        mem._internal_creeps = None; // This is deserialized separately in JS

        RESET_SOURCE_ASSIGNMENTS.with_borrow_mut(|reset| {
            if *reset {
                mem.source_assignments = HashMap::new();
                *reset = false;

                info!("Reset source assignments by command");
            }
        });

        mem
    }

    pub fn screeps_serialize(&mut self) {
        #[allow(deprecated)]
        let new_internal_creeps = Reflect::get(&screeps::memory::ROOT, &JsString::from("creeps")).ok();
        let new_internal_creeps: Option<serde_json::Value> = new_internal_creeps.map(|x| serde_wasm_bindgen::from_value(x).unwrap());
        self._internal_creeps = new_internal_creeps;

        self.periodic_cleanup();

        let mem = serde_json::to_string(&self).unwrap();
        screeps::raw_memory::set(&JsString::from(mem));
    }

    pub fn cleanup_creep(&mut self, name: &str) {
        info!("Cleaning up dead creep {}", name);

        self.machines.creeps.remove(name);
        self.creeps.remove(name);

        self.last_alive_creeps.remove(name);
        self.movement.creeps_data.remove(name);

        for source_assignment in self.source_assignments.values_mut() {
            source_assignment.remove(name);
        }
    }

    pub fn periodic_cleanup(&mut self) {
        let alive_creeps: HashSet<_> = game::creeps().keys().collect();
        let dead_creeps: HashSet<_> = self.last_alive_creeps.difference(&alive_creeps).cloned().collect();

        for dead_creep in &dead_creeps {
            self.cleanup_creep(dead_creep);
        }

        if let Some(internal_creeps) = &mut self._internal_creeps {
            let internal_creeps = internal_creeps.as_object_mut().unwrap();

            let alive_creeps: HashSet<_> = internal_creeps.keys().cloned().collect();
            let dead_creeps: HashSet<_> = alive_creeps.difference(&alive_creeps).cloned().collect();
            for dead_creep in &dead_creeps {
                internal_creeps.remove(dead_creep);
            }
        }

        self.last_alive_creeps = alive_creeps;
    }

    pub fn get_average_tick_rate_over(&self, tick_count: usize) -> f64 {
        self.tick_times.iter().take(tick_count).sum::<f64>() / (tick_count.min(self.tick_times.len()) as f64)
    }
}