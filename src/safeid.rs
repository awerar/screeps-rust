use std::{collections::{HashMap, HashSet}, fmt::Debug, hash::Hash, ops::Deref};

use screeps::{Creep, HasId, MaybeHasId, ObjectId, Source, StructureContainer, StructureController, StructureSpawn};
use serde::{Deserialize, Deserializer, Serialize};
use wasm_bindgen::JsCast;

pub trait IDKind: Clone + Copy + for<'de> Deserialize<'de> + Serialize + Hash + PartialEq + Eq + PartialOrd + Ord + Debug {
    type ID<T: Clone>: Clone + Serialize + Hash + PartialEq + Eq + PartialOrd + Ord + Debug;
}

#[derive(Clone, Copy, Deserialize, Serialize, Hash, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct SafeIDs {}
impl IDKind for SafeIDs {
    type ID<T: Clone> = SafeID<T>;
}

#[derive(Clone, Copy, Deserialize, Serialize, Hash, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct UnsafeIDs {}
impl IDKind for UnsafeIDs {
    type ID<T: Clone> = ObjectId<T>;
}

pub type UnsafeID<T> = ObjectId<T>;
pub struct SafeID<T> {
    pub id: ObjectId<T>,
    entity: T
}

impl<T> Debug for SafeID<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.id.fmt(f)
    }
}

impl<T: Clone> Clone for SafeID<T> {
    fn clone(&self) -> Self {
        Self { id: self.id.clone(), entity: self.entity.clone() }
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

pub trait ToSafeID<T> { fn to_safe_id(self) -> Option<SafeID<T>>; }
impl<T: JsCast + MaybeHasId> ToSafeID<T> for ObjectId<T> {
    fn to_safe_id(self) -> Option<SafeID<T>> {
        self.resolve().map(|entity| SafeID { id: self, entity })
    }
}

trait NormalEntity {}
impl NormalEntity for StructureSpawn {}
impl NormalEntity for StructureContainer {}
impl NormalEntity for StructureController {}
impl NormalEntity for Source {}

pub trait GetSafeID: Sized + Clone { fn safe_id(&self) -> SafeID<Self>; }
impl<T: Sized + Clone + HasId + NormalEntity> GetSafeID for T {
    fn safe_id(&self) -> SafeID<Self> {
        SafeID { id: self.id(), entity: self.clone() }
    }
}

impl GetSafeID for Creep {
    fn safe_id(&self) -> SafeID<Self> {
        let Some(id) = self.try_id() else { panic!("Creep doesn't have ID yet") };
        SafeID { id: id, entity: self.clone() }
    }
}

pub trait FromUnsafe {
    type Unsafe;

    fn from_unsafe(us: Self::Unsafe) -> Self;
}

pub trait TryFromUnsafe: Sized {
    type Unsafe;
    
    fn try_from_unsafe(us: Self::Unsafe) -> Option<Self>;
}

impl<S: FromUnsafe> TryFromUnsafe for S {
    type Unsafe = S::Unsafe;

    fn try_from_unsafe(us: Self::Unsafe) -> Option<Self> {
        Some(Self::from_unsafe(us))
    }
}

impl<T: JsCast + MaybeHasId + GetSafeID> TryFromUnsafe for SafeID<T> {
    type Unsafe = ObjectId<T>;

    fn try_from_unsafe(us: Self::Unsafe) -> Option<Self> {
        Some(us.resolve()?.safe_id())
    }
}

impl<'de, S: Deserialize<'de>> FromUnsafe for S {
    type Unsafe = S;

    fn from_unsafe(us: Self::Unsafe) -> Self {
        us
    }
}

pub trait MakeSafe<S> {
    fn make_safe(self) -> S;
}

impl<S: FromUnsafe> MakeSafe<S> for S::Unsafe {
    fn make_safe(self) -> S {
        S::from_unsafe(self)
    }
}

pub trait TryMakeSafe<S> {
    fn try_make_safe(self) -> Option<S>;
}

impl<S: TryFromUnsafe> TryMakeSafe<S> for S::Unsafe {
    fn try_make_safe(self) -> Option<S> {
        S::try_from_unsafe(self)
    }
}

pub fn deserialize_prune_hashet<'de, D, T>(deserializer: D) -> Result<HashSet<T>, D::Error>
where
    D : Deserializer<'de>,
    T: TryFromUnsafe + Hash + Eq,
    T::Unsafe : Deserialize<'de>
{
    let raw = Vec::<T::Unsafe>::deserialize(deserializer)?;
    Ok(raw.into_iter().filter_map(|u| T::try_from_unsafe(u)).collect())
}

pub fn deserialize_prune_hashmap_keys<'de, D, K, V>(deserializer: D) -> Result<HashMap<K, V>, D::Error> 
where
    D : Deserializer<'de>,
    K: TryFromUnsafe + Eq + Hash,
    K::Unsafe : Deserialize<'de> + Eq + Hash,
    V: Deserialize<'de>
{
    let raw = HashMap::<K::Unsafe, V>::deserialize(deserializer)?;
    Ok(raw.into_iter().filter_map(|(k, v)| K::try_from_unsafe(k).map(|k| (k, v))).collect())
}

pub fn deserialize_prune_hashmap<'de, D, K, V>(deserializer: D) -> Result<HashMap<K, V>, D::Error>
where
    D : Deserializer<'de>,
    K: TryFromUnsafe + Eq + Hash,
    K::Unsafe: Deserialize<'de> + Eq + Hash,
    V: TryFromUnsafe,
    V::Unsafe: Deserialize<'de>,
{
    let raw = HashMap::<K::Unsafe, V::Unsafe>::deserialize(deserializer)?;
    Ok(raw.into_iter().filter_map(|(k, v)| K::try_from_unsafe(k).and_then(|k| V::try_from_unsafe(v).map(|v| (k, v)))).collect())
}