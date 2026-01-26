use std::{cell::RefCell, collections::{HashMap, HashSet}, ops::Add};

use itertools::Itertools;
use js_sys::{JsString, Reflect};
use log::*;
use screeps::{
    Position, Room, StructureController, StructureSpawn, Terrain, find, game, local::ObjectId, objects::{Creep, Source}, prelude::*
};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;
use serde_json_any_key::*;

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
}

#[derive(Serialize, Deserialize)]
pub enum Role {
    Worker(HarvesterData)
}

#[derive(Serialize, Deserialize)]
pub enum HarvesterTarget {
    Controller(ObjectId<StructureController>), Spawn(ObjectId<StructureSpawn>)
}

#[derive(Serialize, Deserialize)]
pub struct HarvesterData {
    pub harvesting: bool,
    pub target: Option<HarvesterTarget>
}

#[derive(Serialize, Deserialize, Debug)]
pub struct HarvestPositionData {
    pub capacity: usize,
    pub assigned: HashSet<String>
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SourceData(#[serde(with = "any_key_map")] HashMap<Position, HarvestPositionData>);

impl SourceData {
    pub fn try_assign(&mut self, creep: &Creep) -> Option<Position> {
        let free_pos = self.0.iter()
            .map(|(pos, pos_data)| (pos_data.capacity - pos_data.assigned.len(), pos))
            .filter(|(free_space, _)| *free_space > 0)
            .sorted()
            .map(|(_, pos)| pos)
            .next()?.clone();

        self.0.get_mut(&free_pos).unwrap().assigned.insert(creep.name());
        Some(free_pos)
    }
}

#[derive(Serialize, Deserialize)]
pub struct SourceDistribution {
    #[serde(with = "any_key_map")] 
    pub harvest_positions: HashMap<ObjectId<Source>, SourceData>,
    pub creep_assignments: HashMap<String, (Position, ObjectId<Source>)>
}

impl SourceDistribution {
    pub fn new(room: Room) -> SourceDistribution {
        let harvest_positions = room.find(find::SOURCES, None).into_iter().map(|source| {
            let free_positions: Vec<_> = 
                (-1..=1).cartesian_product(-1..=1)
                .map(|offset| source.pos().add(offset))
                .filter(|pos| room.get_terrain().get_xy(pos.xy()) != Terrain::Wall).collect();

            let source_data = SourceData(
                free_positions.into_iter()
                    .map(|pos| (pos, HarvestPositionData { assigned: HashSet::new(), capacity: 2 }))
                    .collect()
            );

            (source.id(), source_data)
        }).collect();

        Self { harvest_positions, creep_assignments: HashMap::new() }
    }

    pub fn default() -> SourceDistribution {
        Self::new(game::spawns().values().next().expect("There should be at least one spawn").room().unwrap())
    }

    pub fn get_assignmemnt(&mut self, creep: &Creep) -> Option<(Position, ObjectId<Source>)> {
        if let Some(assignment) = self.creep_assignments.get(&creep.name()) { return Some(assignment.clone()) };

        let mut assignment = None;
        for (source, harvest_positions) in self.harvest_positions.iter_mut() {
            assignment = harvest_positions.try_assign(creep).map(|pos| (pos, source.clone()));
            if assignment.is_some() { break; }
        }

        if let Some(assignment) = assignment {
            info!("Assigning {} to source {}, pos={}", creep.name(), assignment.1, assignment.0);

            self.creep_assignments.insert(creep.name(), assignment);
            self.creep_assignments.get(&creep.name()).cloned()
        } else { None }
    }

    pub fn max_creeps(&self) -> usize {
        self.harvest_positions.values()
            .flat_map(|source_data| source_data.0.values())
            .map(|harvest_pos| harvest_pos.capacity)
            .sum()
    }

    pub fn cleanup_dead_creep(&mut self, dead_creep: &str) {
        self.creep_assignments.remove(dead_creep);

        for source_data in self.harvest_positions.values_mut() {
            for harvest_data in source_data.0.values_mut() {
                harvest_data.assigned.remove(dead_creep);
            }
        }
    }
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

thread_local! {
    static RESET_MEMORY: RefCell<bool> = RefCell::new(false);
}

#[wasm_bindgen]
pub fn reset_memory() {
    RESET_MEMORY.replace(true);
}