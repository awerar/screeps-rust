use std::{collections::{HashMap, HashSet, VecDeque}, marker::PhantomData};

use js_sys::{JsString, Reflect};
use log::warn;
use screeps::{Creep, MaybeHasId, ObjectId, Position, RoomName, game};

use serde::{Deserialize, Serialize};

use crate::{callbacks::Callbacks, colony::Colonies, creeps::{CreepData, fabricator::FabricatorCoordinator, truck::TruckCoordinator}, id::{IDMode, Resolved, Unresolved}, messages::Messages, movement::Movement};

extern crate serde_json_path_to_error as serde_json;

#[derive(Serialize, Deserialize)]
pub struct Memory<M: IDMode> {
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
    pub creeps: HashMap<ObjectId<Creep>, CreepData>,
    pub colonies: Colonies,

    pub incoming_creeps: Vec<(String, CreepData)>,
    pub callbacks: Callbacks,
    pub movement: Movement,
    pub claim_requests: ClaimRequests,
    pub truck_coordinators: HashMap<RoomName, TruckCoordinator>,
    pub fabricator_coordinators: HashMap<RoomName, FabricatorCoordinator>,

    pub messages: Messages,

    pub phantom: PhantomData<M>,
}

impl<M : IDMode> Default for Memory<M> {
    fn default() -> Self {
        Self { 
            _internal_creeps: Default::default(), 
            _alliance_allies: Default::default(), 
            _alliance_my_data: Default::default(), 
            _alliance_allies_data: Default::default(), 
            tick_times: Default::default(), 
            creeps: Default::default(), 
            colonies: Default::default(), 
            incoming_creeps: Default::default(), 
            callbacks: Default::default(), 
            movement: Default::default(), 
            claim_requests: Default::default(), 
            truck_coordinators: Default::default(), 
            fabricator_coordinators: Default::default(), 
            messages: Default::default(), 
            phantom: Default::default() 
        }
    }
}

pub type ClaimRequests = HashSet<Position>;

impl Memory<Unresolved> {
    #[expect(clippy::used_underscore_binding)]
    pub fn screeps_deserialize() -> Self {
        let mem = screeps::raw_memory::get();
        let mut mem = serde_json::from_str(&String::from(mem)).unwrap_or_else(|_| {
            warn!("Unable to parse raw memory. Resetting memory");
            Memory::<Unresolved>::default()
        });

        mem._internal_creeps = None; // This is deserialized separately in JS
        mem
    }

    pub fn resolve(self) -> Memory<Resolved> {
        Memory::<Resolved> {
            _internal_creeps: self._internal_creeps, 
            _alliance_allies: self._alliance_allies, 
            _alliance_my_data: self._alliance_my_data, 
            _alliance_allies_data: self._alliance_allies_data, 
            tick_times: self.tick_times, 
            creeps: self.creeps, 
            colonies: self.colonies, 
            incoming_creeps: self.incoming_creeps, 
            callbacks: self.callbacks, 
            movement: self.movement, 
            claim_requests: self.claim_requests, 
            truck_coordinators: self.truck_coordinators, 
            fabricator_coordinators: self.fabricator_coordinators, 
            messages: self.messages, 
            phantom: PhantomData
        }
    }
}

impl Memory<Resolved> {
    #[expect(clippy::used_underscore_binding)]
    pub fn screeps_serialize(&mut self) {
        #[allow(deprecated)]
        let new_internal_creeps = Reflect::get(&screeps::memory::ROOT, &JsString::from("creeps")).ok();
        let new_internal_creeps: Option<serde_json::Value> = new_internal_creeps.map(|x| serde_wasm_bindgen::from_value(x).unwrap());
        self._internal_creeps = new_internal_creeps;

        self.periodic_cleanup();

        let mem = serde_json::to_string(&self).unwrap();
        screeps::raw_memory::set(&JsString::from(mem));
    }

    pub fn cleanup_creep(&mut self, creep: ObjectId<Creep>) {
        self.creeps.remove(&creep);
        //TODO //self.movement.creeps_data.remove(name);
        //TODO //self.messages.remove(name);
    }

    #[expect(clippy::used_underscore_binding)]
    pub fn periodic_cleanup(&mut self) {
        let alive_creeps: HashSet<_> = game::creeps().values().map(|creep| creep.try_id().unwrap()).collect();
        let dead_creeps: HashSet<_> = self.creeps.keys().cloned().collect::<HashSet<_>>().difference(&alive_creeps).cloned().collect();

        for dead_creep in dead_creeps {
            self.cleanup_creep(dead_creep);
        }

        if let Some(internal_creeps) = &mut self._internal_creeps {
            if let Some(internal_creeps) = internal_creeps.as_object_mut() {
                let alive_creeps: HashSet<_> = internal_creeps.keys().cloned().collect();
                let dead_creeps: HashSet<_> = alive_creeps.difference(&alive_creeps).cloned().collect();
                for dead_creep in &dead_creeps {
                    internal_creeps.remove(dead_creep);
                }
            }
        }
    }

    pub fn get_average_tick_rate_over(&self, tick_count: usize) -> f64 {
        self.tick_times.iter().take(tick_count).sum::<f64>() / (tick_count.min(self.tick_times.len()) as f64)
    }
}