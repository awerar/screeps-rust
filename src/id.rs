use std::{hash::Hash, ops::Deref};

use screeps::{MaybeHasId, ObjectId};
use serde::{Deserialize, Serialize, de::{DeserializeOwned, Error}};
use wasm_bindgen::JsCast;

pub trait IDMode: PartialEq + Eq + Hash + Clone + Default {
    type Wrap<T: JsCast + MaybeHasId>: Eq + Hash + Serialize + DeserializeOwned;
}

#[derive(PartialEq, Eq, Hash, Clone, Default, Serialize, Deserialize)]
pub struct Unresolved;
impl IDMode for Unresolved {
    type Wrap<T: JsCast + MaybeHasId> = ObjectId<T>;
}

#[derive(PartialEq, Eq, Hash, Clone, Default, Serialize, Deserialize)]
pub struct Resolved;
impl IDMode for Resolved {
    type Wrap<T: JsCast + MaybeHasId> = ResolvedId<T>;
}

pub trait IDMaybeResolvable {
    type Target;

    fn try_id_resolve(self) -> Option<Self::Target>;
}

pub trait IDResolvable {
    type Target;

    fn id_resolve(self) -> Self::Target;
}

impl<T> IDMaybeResolvable for T where T : IDResolvable {
    type Target = T::Target;

    fn try_id_resolve(self) -> Option<Self::Target> {
        Some(self.id_resolve())
    }
}

#[derive(Clone)]
pub struct ResolvedId<T> {
    pub inner: T,
    pub id: ObjectId<T>
}

impl<T: JsCast + MaybeHasId> IDMaybeResolvable for ObjectId<T> {
    type Target = ResolvedId<T>;

    fn try_id_resolve(self) -> Option<Self::Target> {
        Some(ResolvedId {
            inner: self.resolve()?,
            id: self
        })
    }
}

impl<T: MaybeHasId> From<T> for ResolvedId<T> {
    fn from(value: T) -> Self {
        ResolvedId { id: value.try_id().unwrap(), inner: value }
    }
}

impl<T: MaybeHasId + Clone> From<&T> for ResolvedId<T> {
    fn from(value: &T) -> Self {
        value.clone().into()
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

impl<'de, T: MaybeHasId + JsCast> Deserialize<'de> for ResolvedId<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de> {
        let id = ObjectId::<T>::deserialize(deserializer)?;
        let id = id.try_id_resolve().ok_or_else(|| D::Error::custom("failed to resolve ObjectId"))?;
        Ok(id)
    }
}