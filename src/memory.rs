use std::{cell::RefCell, collections::{HashMap, HashSet}};

use js_sys::{JsString, Reflect};
use log::*;
use screeps::{Position, RoomName, game};

use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

use crate::{callbacks::Callbacks, colony::{ColonyConfig, ColonyState}, creeps::{CreepRole, harvester::SourceAssignments}, movement::Movement, remote_build::RemoteBuildRequests};

extern crate serde_json_path_to_error as serde_json;

#[derive(Serialize, Deserialize)]
pub struct Memory {
    #[serde(rename = "creeps")]
    _internal_creeps: Option<serde_json::Value>,

    #[serde(default, rename = "creeps_data")]
    pub creeps: HashMap<String, CreepRole>,

    #[serde(default)]
    pub colonies: HashMap<RoomName, (ColonyConfig, ColonyState)>,

    #[serde(default)]
    pub shared: SharedMemory
}

#[derive(Serialize, Deserialize, Default)]
pub struct SharedMemory {
    pub last_alive_creeps: HashSet<String>,
    pub source_assignments: SourceAssignments,
    pub callbacks: Callbacks,
    pub movement: Movement,

    #[serde(default)]
    pub claim_requests: HashSet<Position>,

    #[serde(default)]
    pub remote_build_requests: RemoteBuildRequests
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
    pub fn screeps_deserialize() -> Self {
        RESET_MEMORY.with_borrow_mut(|reset| {
            if *reset {
                screeps::raw_memory::set(&JsString::from("{}"));
                *reset = false;

                info!("Reset memory by command!");
            }
        });

        let memory = screeps::raw_memory::get();
        let mut memory: Memory = serde_json::from_str(&String::from(memory)).expect("Memory should follow correct schema");
        memory._internal_creeps = None; // This is deserialized separately in JS

        RESET_SOURCE_ASSIGNMENTS.with_borrow_mut(|reset| {
            if *reset {
                memory.shared.source_assignments = SourceAssignments::default();
                *reset = false;

                info!("Reset source assignments by command");
            }
        });

        memory
    }

    pub fn screeps_serialize(&mut self) {
        #[allow(deprecated)]
        let new_internal_creeps = Reflect::get(&screeps::memory::ROOT, &JsString::from("creeps")).ok();
        let new_internal_creeps: Option<serde_json::Value> = new_internal_creeps.map(|x| serde_wasm_bindgen::from_value(x).unwrap());
        self._internal_creeps = new_internal_creeps;

        self.periodic_cleanup();

        let memory = serde_json::to_string(&self).unwrap();
        screeps::raw_memory::set(&JsString::from(memory));
    }

    pub fn cleanup_creep(&mut self, name: &str) {
        info!("Cleaning up dead creep {}", name);

        self.creeps.remove(name);
        self.shared.source_assignments.remove(name);
        self.shared.last_alive_creeps.remove(name);
        self.shared.movement.creeps_data.remove(name);
    }

    pub fn periodic_cleanup(&mut self) {
        let alive_creeps: HashSet<_> = game::creeps().keys().collect();
        let dead_creeps: HashSet<_> = self.shared.last_alive_creeps.difference(&alive_creeps).cloned().collect();

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

        self.shared.last_alive_creeps = alive_creeps;
    }
}