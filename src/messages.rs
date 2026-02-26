use std::{collections::{HashMap, HashSet}, hash::Hash, mem};

use itertools::Itertools;
use screeps::{Creep, Position, RoomName, SharedCreepProperties};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::id::{IDMaybeResolvable, IDMode, IDResolvable, Resolved, Unresolved};

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
pub enum SpawnMessage<M: IDMode> {
    SpawnTugboatFor(M::Wrap<Creep>)
}

impl IDMaybeResolvable for SpawnMessage<Unresolved> {
    type Target = SpawnMessage<Resolved>;

    fn try_id_resolve(self) -> Option<Self::Target> {
        Some(match self {
            Self::SpawnTugboatFor(creep_id) => 
                SpawnMessage::SpawnTugboatFor(creep_id.try_id_resolve()?),
        })
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Clone)]
pub enum TruckMessage<M: IDMode> {
    Provider(M::Wrap<Creep>, RoomName),
    Consumer(M::Wrap<Creep>, RoomName),
}

impl TruckMessage<Resolved> {
    pub fn room_name(&self) -> &RoomName {
        match self {
            TruckMessage::Provider(_, room_name) | 
            TruckMessage::Consumer(_, room_name) => room_name,
        }
    }
}

impl IDMaybeResolvable for TruckMessage<Unresolved> {
    type Target = TruckMessage<Resolved>;

    fn try_id_resolve(self) -> Option<Self::Target> {
        Some(match self {
            Self::Provider(creep_id, room_name) => 
                TruckMessage::Provider(creep_id.try_id_resolve()?, room_name),
            Self::Consumer(creep_id, room_name) => 
                TruckMessage::Consumer(creep_id.try_id_resolve()?, room_name),
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

impl<T: IDMaybeResolvable> IDResolvable for Mailbox<T> where T::Target: Eq + Hash {
    type Target = Mailbox<T::Target>;

    fn id_resolve(self) -> Self::Target {
        Mailbox {
            new: self.new.into_iter().filter_map(T::try_id_resolve).collect(),
            readable: self.readable.into_iter().filter_map(T::try_id_resolve).collect(),
        }
    }
}

#[derive(Serialize, Deserialize, Default)]
pub struct Messages<M: IDMode> {
    pub spawn: Mailbox<SpawnMessage<M>>,
    pub trucks: Mailbox<TruckMessage<M>>,

    creeps: HashMap<String, Mailbox<CreepMessage>>,
    creeps_quick: HashMap<String, Mailbox<QuickCreepMessage>>,
}

impl IDResolvable for Messages<Unresolved> {
    type Target = Messages<Resolved>;

    fn id_resolve(self) -> Self::Target {
        Messages { 
            spawn: self.spawn.id_resolve(), 
            trucks: self.trucks.id_resolve(), 
            creeps: self.creeps, 
            creeps_quick: self.creeps_quick 
        }
    }
}

impl Messages<Resolved> {
    pub fn creep(&mut self, creep: &Creep) -> &mut Mailbox<CreepMessage> {
        self.creeps.entry(creep.name()).or_default()
    }

    pub fn creep_quick(&mut self, creep: &Creep) -> &mut Mailbox<QuickCreepMessage> {
        self.creeps_quick.entry(creep.name()).or_default()
    }
}