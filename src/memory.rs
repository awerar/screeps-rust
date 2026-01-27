use std::{cell::RefCell, collections::{HashMap, HashSet}};

use js_sys::{JsString, Reflect};
use log::*;
use screeps::{game};

use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

use crate::{harvester::{HarvesterState, SourceDistribution}, planning::RoadPlan};

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

    #[serde(default = "SourceDistribution::default")]
    pub source_distribution: SourceDistribution,

    #[serde(default)]
    pub road_plan: RoadPlan
}

#[derive(Serialize, Deserialize)]
pub enum Role {
    Worker(HarvesterState)
}

thread_local! {
    static RESET_MEMORY: RefCell<bool> = RefCell::new(false);
    static RESET_PLANNING: RefCell<bool> = RefCell::new(false);
}

#[wasm_bindgen]
pub fn reset_memory() {
    RESET_MEMORY.replace(true);
}

#[wasm_bindgen]
pub fn reset_planning() {
    RESET_PLANNING.replace(true);
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

    RESET_PLANNING.with_borrow_mut(|reset| {
        if *reset {
            memory.road_plan = RoadPlan::default();
            *reset = false;

            info!("Reset road plan by command!");
        }
    });

    memory
}

pub fn serialize_memory(mut memory: Memory) {
    #[allow(deprecated)]
    let new_internal_creeps = Reflect::get(&screeps::memory::ROOT, &JsString::from("creeps")).ok();
    let new_internal_creeps: Option<serde_json::Value> = new_internal_creeps.map(|x| serde_wasm_bindgen::from_value(x).unwrap());
    memory._internal_creeps = new_internal_creeps;

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
            memory.source_distribution.cleanup_dead_creep(&dead_creep);
        }

        #[allow(deprecated)]
        if let Ok(internal_creeps) = Reflect::get(&screeps::memory::ROOT, &JsString::from("creeps")) {
            let internal_creeps_dict: js_sys::Object = internal_creeps.unchecked_into();
            for creep_name_js in js_sys::Object::keys(&internal_creeps_dict).iter() {
                let creep_name = String::from(creep_name_js.dyn_ref::<JsString>().unwrap());

                if !alive_creeps.contains(&creep_name) {
                    info!("Internally cleaning up dead creep {}", creep_name);
                    let _ = Reflect::delete_property(&internal_creeps_dict, &creep_name_js);
                }
            }
        }

        memory.last_alive_creeps = alive_creeps;
    }
}