use std::{collections::{HashMap, HashSet}, hash::Hash, mem};

use itertools::Itertools;
use screeps::{Creep, Position, RoomName, SharedCreepProperties};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::checked_id::{CheckIDs, CheckedID, TryCheckIDs};

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
pub enum SpawnMessage {
    SpawnTugboatFor(CheckedID<Creep>)
}

impl TryCheckIDs for SpawnMessage {
    fn try_check_ids(self) -> Option<Self> {
        Some(match self {
            Self::SpawnTugboatFor(id) => Self::SpawnTugboatFor(id.try_check_ids()?),
        })
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Clone)]
pub enum TruckMessage {
    Provider(CheckedID<Creep>, RoomName),
    Consumer(CheckedID<Creep>, RoomName),
}

impl TruckMessage {
    pub fn room_name(&self) -> &RoomName {
        match self {
            TruckMessage::Provider(_, room_name) | 
            TruckMessage::Consumer(_, room_name) => room_name,
        }
    }
}

impl TryCheckIDs for TruckMessage {
    fn try_check_ids(self) -> Option<Self> {
        Some(match self {
            Self::Provider(id, room_name) => Self::Provider(id.try_check_ids()?, room_name),
            Self::Consumer(id, room_name) => Self::Consumer(id.try_check_ids()?, room_name),
        })
    }
}

#[derive(Serialize, Deserialize)]
#[serde(bound = "T: Eq + Hash + Serialize + DeserializeOwned")]
pub struct Mailbox<T> {
    new: HashSet<T>,
    readable: HashSet<T>
}

impl<T> Default for Mailbox<T> {
    fn default() -> Self {
        Self { new: HashSet::default(), readable: HashSet::default() }
    }
}

impl<T> Mailbox<T> where T : Eq + Hash + Clone + Serialize + DeserializeOwned {
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

impl<T: TryCheckIDs + Eq + Hash> CheckIDs for Mailbox<T> {
    fn check_ids(mut self) -> Self {
        self.new = self.new.into_iter().filter_map(|msg| msg.try_check_ids()).collect();
        self.readable = self.readable.into_iter().filter_map(|msg| msg.try_check_ids()).collect();
        
        self
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

impl CheckIDs for Messages {
    fn check_ids(mut self) -> Self {
        self.spawn = self.spawn.check_ids();
        self.trucks = self.trucks.check_ids();

        self
    }
}