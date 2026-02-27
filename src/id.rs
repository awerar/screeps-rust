use std::{fmt::Debug, hash::Hash, ops::Deref};

use anyhow::anyhow;
use screeps::{HasId, MaybeHasId, ObjectId};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use wasm_bindgen::JsCast;

pub trait IDMode: PartialEq + Eq + Hash + Clone + Default {
    type Wrap<T: JsCast + MaybeHasId + Clone + Debug>: Eq + Hash + Serialize + DeserializeOwned + Clone + Debug;
}

#[derive(PartialEq, Eq, Hash, Clone, Default, Serialize, Deserialize)]
pub struct Unresolved;
impl IDMode for Unresolved {
    type Wrap<T: JsCast + MaybeHasId + Clone + Debug> = ObjectId<T>;
}

#[derive(PartialEq, Eq, Hash, Clone, Default, Serialize, Deserialize)]
pub struct Resolved;
impl IDMode for Resolved {
    type Wrap<T: JsCast + MaybeHasId + Clone + Debug> = ResolvedId<T>;
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

impl<T: Clone> ResolvedId<T> {
    pub fn cloned(&self) -> T {
        self.inner.clone()
    }
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

pub trait IntoResolvedID<T> { fn into_rid(self) -> ResolvedId<T>; }
impl<R: HasId> IntoResolvedID<R> for R {
    fn into_rid(self) -> ResolvedId<R> {
        ResolvedId { id: self.id(), inner: self }
    }
}

pub trait TryIntoResolvedID<T> { fn try_into_rid(self) -> anyhow::Result<ResolvedId<T>>; }
impl<R: MaybeHasId> TryIntoResolvedID<R> for R {
    fn try_into_rid(self) -> anyhow::Result<ResolvedId<R>> {
        Ok(ResolvedId { id: self.try_id().ok_or(anyhow!("Unable to get id"))?, inner: self })
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

impl<T: Debug> Debug for ResolvedId<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.inner.fmt(f)
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
    fn deserialize<D>(_: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de> {
        unimplemented!("ResolvedId's are not meant to be deserialized. This is just here for convenient Deserialize derives.")
    }
}