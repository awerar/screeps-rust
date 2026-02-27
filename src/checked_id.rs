use std::{fmt::Debug, hash::Hash, ops::Deref};

use screeps::{Creep, HasId, MaybeHasId, ObjectId};
use serde::{Deserialize, Serialize, ser};
use wasm_bindgen::JsCast;

#[derive(Clone)]
enum CheckedIDStatus<T> {
    Unchecked,
    Valid(T)
}

pub struct CheckedID<T> {
    pub id: ObjectId<T>,
    status: CheckedIDStatus<T>
}

pub trait CheckIDs { fn check_ids(self) -> Self; }
pub trait TryCheckIDs: Sized { fn try_check_ids(self) -> Option<Self>; }

impl<T: CheckIDs> TryCheckIDs for T {
    fn try_check_ids(self) -> Option<Self> {
        Some(self.check_ids())
    }
}

impl<T: JsCast + MaybeHasId> TryCheckIDs for CheckedID<T> {
    fn try_check_ids(mut self) -> Option<Self> {
        if matches!(self.status, CheckedIDStatus::Valid(_)) {
            panic!("ID has already been checked")
        }

        self.status = CheckedIDStatus::Valid(self.id.resolve()?);
        Some(self)
    }
}

pub trait GetCheckedID: Sized + Clone { fn checked_id(&self) -> CheckedID<Self>; }
impl<T: Sized + Clone + HasId> GetCheckedID for T {
    fn checked_id(&self) -> CheckedID<Self> {
        CheckedID { id: self.id(), status: CheckedIDStatus::Valid(self.clone()) }
    }
}

pub trait CreepGetCheckedID: Sized + Clone { fn checked_id(&self) -> CheckedID<Self>; }
impl CreepGetCheckedID for Creep {
    fn checked_id(&self) -> CheckedID<Self> {
        let Some(id) = self.try_id() else { panic!("Creep doesn't have ID yet") };
        CheckedID { id: id, status: CheckedIDStatus::Valid(self.clone()) }
    }
}

impl<T> Deref for CheckedID<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        match self.status {
            CheckedIDStatus::Unchecked => panic!("ID has not been checked"),
            CheckedIDStatus::Valid(ref t) => t,
        }
    }
}

impl<T> Serialize for CheckedID<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: serde::Serializer {
        if matches!(self.status, CheckedIDStatus::Unchecked) {
            return Err(ser::Error::custom("Trying to serialize unchecked ID"))
        }

        self.id.serialize(serializer)
    }
}

impl<'de, T> Deserialize<'de> for CheckedID<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where D: serde::Deserializer<'de> {
        let id = ObjectId::<T>::deserialize(deserializer)?;
        Ok(Self { id, status: CheckedIDStatus::Unchecked })
    }
}

impl<T> Eq for CheckedID<T> {}
impl<T> PartialEq for CheckedID<T> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl<T> Hash for CheckedID<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl<T> Ord for CheckedID<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.id.cmp(&other.id)
    }
}

impl<T> PartialOrd for CheckedID<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.id.partial_cmp(&other.id)
    }
}

impl<T: Clone> Clone for CheckedID<T> {
    fn clone(&self) -> Self {
        Self { id: self.id.clone(), status: self.status.clone() }
    }
}

impl<T> Debug for CheckedID<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.id.fmt(f)
    }
}