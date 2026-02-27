use std::{collections::{HashMap, HashSet}, hash::Hash, mem};

use itertools::Itertools;
use screeps::{Creep, Position, RoomName, SharedCreepProperties};
use serde::{Deserialize, Serialize};

use crate::safeid::{IDKind, SafeIDs, ToSafeID, TryDeserialize, UnsafeIDs, deserialize_prune_hashet};

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Clone, Debug)]
pub enum CreepMessage {
    AssignedTugBoat(String),
    TruckTarget
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Clone, Debug)]
pub enum QuickCreepMessage {
    TuggedRequestMove { target: Position, range: u32 },
    TugMove,
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Clone)]
pub enum SpawnMessage<I: IDKind = SafeIDs> {
    SpawnTugboatFor(I::ID<Creep>)
}

impl TryDeserialize for SpawnMessage {
    fn try_deserialize<'de, D : serde::Deserializer<'de>>(deserializer: D) -> Result<Option<Self>, D::Error> {
        let raw = SpawnMessage::<UnsafeIDs>::deserialize(deserializer)?;
        Ok(match raw {
            SpawnMessage::SpawnTugboatFor(id) => id.to_safe_id().map(Self::SpawnTugboatFor),
        })
    }
}

#[derive(Deserialize, Serialize, PartialEq, Eq, Hash, Clone)]
pub enum TruckMessage<I: IDKind = SafeIDs> {
    Provider(I::ID<Creep>, RoomName),
    Consumer(I::ID<Creep>, RoomName),
}

impl TryDeserialize for TruckMessage {
    fn try_deserialize<'de, D : serde::Deserializer<'de>>(deserializer: D) -> Result<Option<Self>, D::Error> {
        let raw = TruckMessage::<UnsafeIDs>::deserialize(deserializer)?;
        Ok(match raw {
            TruckMessage::Provider(id, x) => id.to_safe_id().map(|id| Self::Provider(id, x)),
            TruckMessage::Consumer(id, x) => id.to_safe_id().map(|id| Self::Consumer(id, x)),
        })
    }
}

impl TruckMessage {
    pub fn room_name(&self) -> &RoomName {
        match self {
            TruckMessage::Provider(_, room_name) | 
            TruckMessage::Consumer(_, room_name) => room_name,
        }
    }
}


#[derive(Serialize, Deserialize)]
#[serde(bound = "T: Hash + Eq + Serialize + TryDeserialize")]
pub struct Mailbox<T> {
    #[serde(deserialize_with = "deserialize_prune_hashet")] new: HashSet<T>,
    #[serde(deserialize_with = "deserialize_prune_hashet")] readable: HashSet<T>,
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

    pub fn empty(&self) -> bool {
        self.readable.is_empty()
    }
}

#[derive(Serialize, Deserialize, Default)]
pub struct Messages {
    pub spawn: Mailbox<SpawnMessage>,
    pub trucks: Mailbox<TruckMessage>,

    creeps: HashMap<String, Mailbox<CreepMessage>>,
    creeps_quick: HashMap<String, Mailbox<QuickCreepMessage>>,
}

impl Messages where {
    pub fn creep(&mut self, creep: &Creep) -> &mut Mailbox<CreepMessage> {
        self.creeps.entry(creep.name()).or_default()
    }

    pub fn creep_quick(&mut self, creep: &Creep) -> &mut Mailbox<QuickCreepMessage> {
        self.creeps_quick.entry(creep.name()).or_default()
    }
}