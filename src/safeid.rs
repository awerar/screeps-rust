use std::{collections::{HashMap, HashSet}, fmt::Debug, hash::Hash, ops::Deref};

use derive_deref::{Deref, DerefMut};
use screeps::{Creep, HasId, MaybeHasId, ObjectId};
use serde::{Deserialize, Deserializer, Serialize};
use wasm_bindgen::JsCast;

pub trait IDKind {
    type ID<T>;
}

#[derive(Clone, Copy, Deserialize, Serialize, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct SafeIDs {}
impl IDKind for SafeIDs {
    type ID<T> = SafeID<T>;
}

pub struct UnsafeIDs {}
impl IDKind for UnsafeIDs {
    type ID<T> = ObjectId<T>;
}

#[derive(Clone, Debug)]
pub struct SafeID<T> {
    pub id: ObjectId<T>,
    entity: T
}

pub trait TryDeserialize: Sized {
    fn try_deserialize<'de, D : Deserializer<'de>>(deserializer: D) -> Result<Option<Self>, D::Error>;
}

impl<T> TryDeserialize for T where T: for<'de> Deserialize<'de> {
    fn try_deserialize<'de1, D : Deserializer<'de1>>(deserializer: D) -> Result<Option<Self>, D::Error> {
        Ok(Some(Self::deserialize(deserializer)?))
    }
}

impl<T: JsCast + MaybeHasId> TryDeserialize for SafeID<T> {
    fn try_deserialize<'de, D>(deserializer: D) -> Result<Option<Self>, D::Error>
        where D: Deserializer<'de>
    {
        Ok(ObjectId::<T>::deserialize(deserializer)?.to_safe_id())
    }
}

pub trait ToSafeID<T> { fn to_safe_id(self) -> Option<SafeID<T>>; }
impl<T: JsCast + MaybeHasId> ToSafeID<T> for ObjectId<T> {
    fn to_safe_id(self) -> Option<SafeID<T>> {
        self.resolve().map(|entity| SafeID { id: self, entity })
    }
}

pub trait GetSafeID: Sized + Clone { fn safe_id(&self) -> SafeID<Self>; }
impl<T: Sized + Clone + HasId> GetSafeID for T {
    fn safe_id(&self) -> SafeID<Self> {
        SafeID { id: self.id(), entity: self.clone() }
    }
}

pub trait CreepGetSafeID: Sized + Clone { fn safe_id(&self) -> SafeID<Self>; }
impl CreepGetSafeID for Creep {
    fn safe_id(&self) -> SafeID<Self> {
        let Some(id) = self.try_id() else { panic!("Creep doesn't have ID yet") };
        SafeID { id: id, entity: self.clone() }
    }
}

impl<T> Deref for SafeID<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.entity
    }
}

impl<T> Serialize for SafeID<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: serde::Serializer {
        self.id.serialize(serializer)
    }
}

impl<T> Eq for SafeID<T> {}
impl<T> PartialEq for SafeID<T> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl<T> Hash for SafeID<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl<T> Ord for SafeID<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.id.cmp(&other.id)
    }
}

impl<T> PartialOrd for SafeID<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.id.partial_cmp(&other.id)
    }
}

#[derive(Deref, DerefMut, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct DeserializeOption<T>(pub Option<T>);

impl<'de, T: TryDeserialize> Deserialize<'de> for DeserializeOption<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(DeserializeOption(T::try_deserialize(deserializer)?))
    }
}


pub fn deserialize_prune_hashet<'de, D : Deserializer<'de>, T: TryDeserialize + Eq + Hash>(deserializer: D) -> Result<HashSet<T>, D::Error> {
    let raw = Vec::<DeserializeOption::<T>>::deserialize(deserializer)?;
    Ok(raw.into_iter().filter_map(|x| x.0).collect())
}

pub fn deserialize_prune_hashmap<'de, D : Deserializer<'de>, K: TryDeserialize + Eq + Hash, V : Deserialize<'de>>(deserializer: D) -> Result<HashMap<K, V>, D::Error> {
    let raw = HashMap::<DeserializeOption::<K>, V>::deserialize(deserializer)?;
    Ok(raw.into_iter().filter_map(|(k, v)| k.0.map(|k| (k, v))).collect())
}