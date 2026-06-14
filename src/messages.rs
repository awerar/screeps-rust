use std::{collections::{HashMap, HashSet}, hash::Hash, mem};

use itertools::Itertools;
use screeps::Creep;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::safeid::{SafeID, TryFromUnsafe, deserialize_prune_hashmap_keys, deserialize_prune_hashset};

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Clone, Debug)]
pub enum CreepMessage {
    TruckTarget
}

impl TryFromUnsafe for CreepMessage {
    type Unsafe = Self;

    fn try_from_unsafe(us: Self::Unsafe) -> Option<Self> {
        Some(us)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(bound = "T: Serialize + TryFromUnsafe + Hash + Eq, T::Unsafe : DeserializeOwned")]
pub struct Mailbox<T> {
    #[serde(deserialize_with = "deserialize_prune_hashset")] new: HashSet<T>,
    #[serde(deserialize_with = "deserialize_prune_hashset")] readable: HashSet<T>,
}

impl<T> Default for Mailbox<T> {
    fn default() -> Self {
        Self { new: HashSet::default(), readable: HashSet::default() }
    }
}

impl<T: Eq + Hash + Clone> Mailbox<T> {
    pub fn flush(&mut self) {
        self.readable = mem::take(&mut self.new);
    }

    pub fn read_all(&self) -> Vec<T> {
        self.readable.iter().cloned().collect_vec()
    }

    #[expect(clippy::needless_pass_by_value)]
    pub fn read(&self, msg: T) -> bool {
        self.readable.contains(&msg)
    }

    pub fn send(&mut self, msg: T) {
        self.new.insert(msg);
    }
}

#[derive(Serialize, Deserialize, Default)]
pub struct Messages {
#[serde(deserialize_with = "deserialize_prune_hashmap_keys")] 
    creeps: HashMap<SafeID<Creep>, Mailbox<CreepMessage>>,
}

impl Messages where {
    pub fn creep(&mut self, creep: &SafeID<Creep>) -> &mut Mailbox<CreepMessage> {
        self.creeps.entry(creep.clone()).or_default()
    }
}