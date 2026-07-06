use std::{collections::HashMap, sync::LazyLock};

use screeps::{game};
use serde::{Deserialize, Serialize};

use crate::{memory::Memory, colony::update_colonies};

#[derive(Hash, PartialEq, Eq, Deserialize, Serialize, Clone)]
enum PeriodicCallback {
    RoomUpdate
}

static PERIODIC_CALLBACKS: LazyLock<HashMap<PeriodicCallback, u32>> = LazyLock::new(|| {
    HashMap::from([
        ( PeriodicCallback::RoomUpdate, 10 ),
    ])
});

impl PeriodicCallback {
    pub fn execute(&self, mem: &mut Memory) {
        match self {
            PeriodicCallback::RoomUpdate => update_colonies(mem),
        }
    }
}

#[derive(Deserialize, Serialize, Default)]
pub struct Callbacks{ 
    last_periodic: HashMap<PeriodicCallback, u32>
}

impl Memory {
    pub fn handle_callbacks(&mut self) {
        for (callback, delay) in PERIODIC_CALLBACKS.iter() {
            let last_time = self.callbacks.last_periodic.entry(callback.clone())
                .or_insert(0);

            if game::time() < *last_time + *delay { continue; }

            *last_time = game::time();
            callback.execute(self);
        }
    }
}