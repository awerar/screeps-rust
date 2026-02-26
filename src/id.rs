use std::{collections::{HashMap, HashSet}, hash::Hash, ops::Deref};

use screeps::{Creep, MaybeHasId, ObjectId};
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsCast;

use crate::creeps::CreepData;

pub trait IDMode where {
    type Wrap<T>: Eq + Hash;
}

pub struct Unresolved;
impl IDMode for Unresolved {
    type Wrap<T> = ObjectId<T>;
}

pub struct Resolved;
impl IDMode for Resolved {
    type Wrap<T> = ResolvedId<T>;
}

pub struct ResolvedId<T> {
    inner: T,
    id: ObjectId<T>
}

impl<T: JsCast + MaybeHasId> ResolvedId<T> {
    fn resolve(id: ObjectId<T>) -> Option<Self> {
        Some(Self {
            inner: id.resolve()?,
            id
        })
    }
}

impl<T> Deref for ResolvedId<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T> Eq for ResolvedId<T> { }
impl<T> PartialEq for ResolvedId<T> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl<T> Hash for ResolvedId<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl<T: MaybeHasId> Serialize for ResolvedId<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer {
        self.id.serialize(serializer)
    }
}

#[derive(Serialize, Deserialize)]
struct TestMemory<M: IDMode> {
    creep: M::Wrap<Creep>,
    x: u32,
    creeps: HashMap<M::Wrap<Creep>, CreepData>,
}

impl TestMemory<Unresolved> {
    fn resolve(self) -> TestMemory<Resolved> {
        TestMemory::<Resolved> { 
            creep: ResolvedId::resolve(self.creep).unwrap(),
            x: self.x,
            creeps: self.creeps.into_iter()
                .filter_map(|(creed_id, creep_data)| ResolvedId::resolve(creed_id).map(|creep| (creep, creep_data)))
                .collect()
        }
    }
}

fn test() {
    let s: String = "".to_string();

    let unresolved_mem: TestMemory<Unresolved> = serde_json::from_str(&s).unwrap();
    let mem = unresolved_mem.resolve();
    mem.creep.move_direction(screeps::Direction::Bottom);

    let s = serde_json::to_string(&mem).unwrap();
}