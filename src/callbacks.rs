use std::{cmp::Reverse, collections::{BinaryHeap, HashMap}, sync::LazyLock};

use screeps::game;
use serde::{Deserialize, Serialize};

use crate::memory::Memory;

#[derive(Hash, PartialEq, Eq, Deserialize, Serialize, Clone)]
enum PeriodicCallback {
    MemoryCleanup
}

const PERIODIC_CALLBACKS: LazyLock<HashMap<PeriodicCallback, u32>> = LazyLock::new(|| {
    use PeriodicCallback::*;

    HashMap::from([
        ( MemoryCleanup, 100 )
    ])
});

impl PeriodicCallback {
    pub fn execute(&self, memory: &mut Memory) {
        match self {
            PeriodicCallback::MemoryCleanup => memory.periodic_cleanup(),
        }
    }
}

#[derive(PartialEq, Eq, Deserialize, Serialize)]
pub enum Callback {
    CreepCleanup(String)
}

impl Callback {
    pub fn execute(self, memory: &mut Memory) {
        match self {
            Callback::CreepCleanup(creep) => memory.cleanup_creep(&creep),
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
        loop {
            let Some(callback) = self.shared.callbacks.scheduled.peek() else { break };
            if game::time() < callback.0.0 { break; }
            let callback = self.shared.callbacks.scheduled.pop().unwrap();
            callback.1.execute(self);
        }

        for (callback, delay) in PERIODIC_CALLBACKS.iter() {
            let last_time = self.shared.callbacks.last_periodic.entry(callback.clone())
                .or_insert(0);

            if game::time() < *last_time + *delay { continue; }

            *last_time = game::time();
            callback.execute(self);
        }
    }
}