use std::{cell::RefCell, collections::{HashMap, HashSet}};

use js_sys::{JsString, Reflect};
use log::*;
use screeps::game;

use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

use crate::{creeps::Role, harvester::{SourceAssignments}, movement::{MOVEMENT_DATA, MovementData}};

extern crate serde_json_path_to_error as serde_json;

#[derive(Serialize, Deserialize)]
pub struct Memory {
    #[serde(rename = "creeps")]
    _internal_creeps: Option<serde_json::Value>,
    #[serde(default)]
    next_clean_time: u32,

    #[serde(default, rename = "creeps_data")]
    pub creeps: HashMap<String, Role>,
    #[serde(default)]
    pub last_alive_creeps: HashSet<String>,

    #[serde(default)]
    pub source_assignments: SourceAssignments,

    #[serde(default)]
    movement_data: MovementData,

    pub claimer_creep: Option<String>
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

#[wasm_bindgen]
pub fn reset_tile_usage() {
    RESET_TILE_USAGE.replace(true);
}

pub fn deserialize_memory() -> Memory {
    RESET_MEMORY.with_borrow_mut(|reset| {
        if *reset {
            screeps::raw_memory::set(&JsString::from("{}"));
            *reset = false;

            info!("Reset memory by command!");
        }
    });

    let memory = screeps::raw_memory::get();
    let mut memory: Memory = serde_json::from_str(&String::from(memory)).expect("Memory should follow correct schema");
    clean_memory(&mut memory);

    RESET_TILE_USAGE.with_borrow_mut(|reset| {
        if *reset {
            memory.movement_data.tile_usage.clear();
            *reset = false;

            info!("Reset road plan by command!");
        }
    });

    RESET_SOURCE_ASSIGNMENTS.with_borrow_mut(|reset| {
        if *reset {
            memory.source_assignments = SourceAssignments::default();
            *reset = false;

            info!("Reset source assignments by command");
        }
    });

    MOVEMENT_DATA.replace(std::mem::take(&mut memory.movement_data));

    memory
}

pub fn serialize_memory(mut memory: Memory) {
    #[allow(deprecated)]
    let new_internal_creeps = Reflect::get(&screeps::memory::ROOT, &JsString::from("creeps")).ok();
    let new_internal_creeps: Option<serde_json::Value> = new_internal_creeps.map(|x| serde_wasm_bindgen::from_value(x).unwrap());
    memory._internal_creeps = new_internal_creeps;

    memory.movement_data = MOVEMENT_DATA.take();

    let memory = serde_json::to_string(&memory).unwrap();
    screeps::raw_memory::set(&JsString::from(memory));
}

fn clean_memory(memory: &mut Memory) {
    if game::time() >= memory.next_clean_time {
        memory.next_clean_time = game::time() + 100;

        let alive_creeps: HashSet<_> = game::creeps().keys().collect();
        let dead_creeps: HashSet<_> = memory.last_alive_creeps.difference(&alive_creeps).cloned().collect();

        for dead_creep in dead_creeps {
            info!("Cleaning up dead creep {}", dead_creep);

            memory.creeps.remove(&dead_creep);
            memory.source_assignments.remove(&dead_creep);
            memory.movement_data.creeps_data.remove(&dead_creep);
            
            if let Some(claimer_creep) = &memory.claimer_creep {
                if claimer_creep == &dead_creep {
                    memory.claimer_creep = None;
                }
            }
        }

        #[allow(deprecated)]
        if let Ok(internal_creeps) = Reflect::get(&screeps::memory::ROOT, &JsString::from("creeps")) {
            let internal_creeps_dict: js_sys::Object = internal_creeps.unchecked_into();
            if !internal_creeps_dict.is_null_or_undefined() { 
                for creep_name_js in js_sys::Object::keys(&internal_creeps_dict).iter() {
                    let creep_name = String::from(creep_name_js.dyn_ref::<JsString>().unwrap());

                    if !alive_creeps.contains(&creep_name) {
                        info!("Internally cleaning up dead creep {}", creep_name);
                        let _ = Reflect::delete_property(&internal_creeps_dict, &creep_name_js);
                    }
                }
            }
        }

        memory.last_alive_creeps = alive_creeps;
    }
}