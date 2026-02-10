use std::{collections::{HashMap, HashSet}, hash::Hash, mem};

use itertools::Itertools;
use screeps::{Creep, ObjectId, Position, SharedCreepProperties};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Clone, Debug)]
pub enum CreepMessage {
    AssignedTugBoat(String)
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Clone, Debug)]
pub enum QuickCreepMessage {
    TuggedRequestMove { target: Position, range: u32 },
    TugMove,
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Clone)]
pub enum SpawnMessage {
    SpawnTugboatFor(ObjectId<Creep>)
}


#[derive(Serialize, Deserialize)]
#[serde(bound = "T: Eq + Hash + Serialize + DeserializeOwned")]
pub struct Mailbox<T> {
    new: HashSet<T>,
    readable: HashSet<T>
}

impl<T> Default for Mailbox<T> {
    fn default() -> Self {
        Self { new: Default::default(), readable: Default::default() }
    }
}

impl<T> Mailbox<T> where T : Eq + Hash + Clone + Serialize + DeserializeOwned {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn flush(&mut self) {
        self.readable = mem::take(&mut self.new);
    }

    pub fn read_all(&self) -> Vec<T> {
        self.readable.iter().cloned().collect_vec()
    }

    pub fn read(&self, msg: T) -> bool {
        self.readable.contains(&msg)
    }

    pub fn send(&mut self, msg: T) {
        self.new.insert(msg);
    }

    pub fn empty(&self) -> bool {
        self.readable.is_empty()
    }
}

#[derive(Serialize, Deserialize, Default)]
pub struct Messages {
    pub spawn: Mailbox<SpawnMessage>,

    creeps: HashMap<String, Mailbox<CreepMessage>>,
    creeps_quick: HashMap<String, Mailbox<QuickCreepMessage>>,
}

impl Messages where {
    pub fn creep(&mut self, creep: &Creep) -> &mut Mailbox<CreepMessage> {
        self.creeps.entry(creep.name()).or_insert_with(|| Mailbox::new())
    }

    pub fn creep_quick(&mut self, creep: &Creep) -> &mut Mailbox<QuickCreepMessage> {
        self.creeps_quick.entry(creep.name()).or_insert_with(|| Mailbox::new())
    }

    pub fn remove(&mut self, creep: &str) {
        self.creeps.remove(creep);
        self.creeps_quick.remove(creep);
    }
}