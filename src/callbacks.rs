use std::{cmp::Reverse, collections::{BinaryHeap, HashMap}, sync::LazyLock};

use screeps::game;
use serde::{Deserialize, Serialize};

use crate::{memory::Memory, colony::update_rooms};

#[derive(Hash, PartialEq, Eq, Deserialize, Serialize, Clone)]
enum PeriodicCallback {
    MemoryCleanup,
    RoomUpdate,
    RemoteBuildUpdate
}

static PERIODIC_CALLBACKS: LazyLock<HashMap<PeriodicCallback, u32>> = LazyLock::new(|| {
    use PeriodicCallback::*;

    HashMap::from([
        ( MemoryCleanup, 100 ),
        ( RoomUpdate, 10 ),
        ( RemoteBuildUpdate, 5 )
    ])
});

impl PeriodicCallback {
    pub fn execute(&self, mem: &mut Memory) {
        match self {
            PeriodicCallback::MemoryCleanup => mem.periodic_cleanup(),
            PeriodicCallback::RoomUpdate => update_rooms(mem),
            PeriodicCallback::RemoteBuildUpdate => mem.remote_build_requests.update_requests(),
        }
    }
}

#[derive(PartialEq, Eq, Deserialize, Serialize)]
pub enum Callback {
    CreepCleanup(String)
}

impl Callback {
    pub fn execute(self, mem: &mut Memory) {
        match self {
            Callback::CreepCleanup(creep) => mem.cleanup_creep(&creep),
        }
    }
}

#[derive(PartialEq, Eq, Deserialize, Serialize)]
struct ScheduledCallback(Reverse<u32>, Callback);

impl Ord for ScheduledCallback {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.cmp(&other.0)
    }
}

impl PartialOrd for ScheduledCallback {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Deserialize, Serialize, Default)]
pub struct Callbacks{ 
    scheduled: BinaryHeap<ScheduledCallback>,
    last_periodic: HashMap<PeriodicCallback, u32>
}

impl Callbacks {
    pub fn schedule(&mut self, time: u32, callback: Callback) {
        self.scheduled.push(ScheduledCallback(Reverse(time), callback));
    }
}

impl Memory {
    pub fn handle_callbacks(&mut self) {
        while let Some(callback) = self.callbacks.scheduled.peek() {
            if game::time() < callback.0.0 { break; }
            let callback = self.callbacks.scheduled.pop().unwrap();
            callback.1.execute(self);
        }

        for (callback, delay) in PERIODIC_CALLBACKS.iter() {
            let last_time = self.callbacks.last_periodic.entry(callback.clone())
                .or_insert(0);

            if game::time() < *last_time + *delay { continue; }

            *last_time = game::time();
            callback.execute(self);
        }
    }
}