use std::{collections::{HashMap, HashSet}, hash::Hash, mem};

use itertools::Itertools;
use screeps::{Creep, Position, RoomName};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::safeid::{IDKind, SafeID, SafeIDs, TryFromUnsafe, TryMakeSafe, UnsafeIDs, deserialize_prune_hashet, deserialize_prune_hashmap_keys};

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

impl TryFromUnsafe for SpawnMessage {
    type Unsafe = SpawnMessage<UnsafeIDs>;

    fn try_from_unsafe(us: Self::Unsafe) -> Option<Self> {
        Some(match us {
            Self::Unsafe::SpawnTugboatFor(id) => Self::SpawnTugboatFor(id.try_make_safe()?),
        })
    }
}

#[derive(Deserialize, Serialize, PartialEq, Eq, Hash, Clone)]
pub enum TruckMessage<I: IDKind = SafeIDs> {
    Provider(I::ID<Creep>, RoomName),
    Consumer(I::ID<Creep>, RoomName),
}

impl TryFromUnsafe for TruckMessage {
    type Unsafe = TruckMessage<UnsafeIDs>;

    fn try_from_unsafe(us: Self::Unsafe) -> Option<Self> {
        Some(match us {
            Self::Unsafe::Provider(id, x) => Self::Provider(id.try_make_safe()?, x),
            Self::Unsafe::Consumer(id, x) => Self::Consumer(id.try_make_safe()?, x),
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
#[serde(bound = "T: Serialize + TryFromUnsafe + Hash + Eq, T::Unsafe : DeserializeOwned")]
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

    #[serde(deserialize_with = "deserialize_prune_hashmap_keys")] 
    creeps: HashMap<SafeID<Creep>, Mailbox<CreepMessage>>,
    #[serde(deserialize_with = "deserialize_prune_hashmap_keys")] 
    creeps_quick: HashMap<SafeID<Creep>, Mailbox<QuickCreepMessage>>,
}

impl Messages where {
    pub fn creep(&mut self, creep: &SafeID<Creep>) -> &mut Mailbox<CreepMessage> {
        self.creeps.entry(creep.clone()).or_default()
    }

    pub fn creep_quick(&mut self, creep: &SafeID<Creep>) -> &mut Mailbox<QuickCreepMessage> {
        self.creeps_quick.entry(creep.clone()).or_default()
    }
}