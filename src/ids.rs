use std::{fmt::Debug, hash::Hash, ops::Deref, rc::Rc};

use derive_where::derive_where;
use screeps::{Creep, ObjectId, SharedCreepProperties, game};
use serde::{Deserialize, Serialize, Serializer};
use wasm_bindgen::JsCast;

use crate::{check::{DO, Check, CheckFrom}, domain_traits::{HasId, MaybeHasId}};

// TODO: CheckedID currently handles two things:
// Make sure that entities have an id
// Make sure that ids have a entity
// We could split this into two
// struct CheckedId<T> -- Used by for example creeps to always have an id
// Something else to deserialize and serialize things with ids

pub trait IDKind {
    type ID<T>: Serialize + Debug + Clone + PartialEq + Eq + PartialOrd + Ord + Hash;
}

#[derive(Clone, Copy, Deserialize, Serialize, Hash, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct CheckedIDs {}
impl IDKind for CheckedIDs {
    type ID<T> = CheckedID<T>;
}

#[derive(Deserialize, Serialize, Hash, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct UncheckedIDs {}
impl IDKind for UncheckedIDs {
    type ID<T> = ObjectId<T>;
}

#[derive_where(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct CheckedID<T> {
    pub id: ObjectId<T>,
    #[derive_where(skip)] inner: Rc<T>
}

impl<T: HasId> CheckedID<T> {
    pub fn new(entity: T) -> Self {
        CheckedID { id: entity.id(), inner: Rc::new(entity) }
    }
}

impl<T: MaybeHasId> CheckedID<T> {
    pub fn try_new(entity: T) -> Option<Self> {
        Some(CheckedID { id: entity.try_id()?, inner: Rc::new(entity) })
    }
}

impl<T> AsRef<T> for CheckedID<T> {
    fn as_ref(&self) -> &T {
        &self.inner
    }
}

impl<T> Deref for CheckedID<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T> Serialize for CheckedID<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.id.serialize(serializer)
    }
}

impl CheckedID<Creep> {
    pub fn from_name(name: String) -> Option<CheckedID<Creep>> {
        Self::from_creep(game::creeps().get(name)?)
    }

    pub fn from_creep(creep: Creep) -> Option<CheckedID<Creep>> {
        Some(CheckedID { id: creep.try_id()?, inner: Rc::new(creep) })
    }

    pub fn creeps() -> impl Iterator<Item = CheckedID<Creep>> {
        game::creeps().values().filter_map(Self::from_creep)
    }
}

pub trait IntoCheckedID: Sized { fn into_checked(self) -> CheckedID<Self>;}
impl<T: HasId> IntoCheckedID for T {
    fn into_checked(self) -> CheckedID<Self> {
        CheckedID::new(self)
    }
}

pub trait TryIntoCheckedID: Sized { fn try_into_checked(self) -> Option<CheckedID<Self>>;}
impl<T: MaybeHasId> TryIntoCheckedID for T {
    fn try_into_checked(self) -> Option<CheckedID<Self>> {
        CheckedID::try_new(self)
    }
}

impl<T: JsCast + screeps::MaybeHasId> CheckFrom for CheckedID<T> {
    type Unchecked = ObjectId<T>;
    type Err = ();
    
    fn check_from(uc: Self::Unchecked) -> Result<Self, ()> {
        uc.resolve().ok_or(()).map(|entity| CheckedID { id: uc, inner: Rc::new(entity) })
    }
}

#[derive(Serialize, Deserialize)]
#[derive_where(PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Clone)]
#[serde(bound(deserialize = "I::ID<T> : DO", serialize = ""))]
pub struct DumbID<T, I : IDKind = CheckedIDs>(I::ID<T>);

impl<T> From<CheckedID<T>> for DumbID<T> {
    fn from(id: CheckedID<T>) -> Self {
        DumbID::new(id)
    }
}

impl<T> CheckedID<T> {
    pub fn dumb_id(&self) -> DumbID<T> {
        self.clone().into()
    }
}

impl<T> DumbID<T> {
    pub fn new(id: CheckedID<T>) -> Self {
        Self(id)
    }
}

impl DumbID<Creep> {
    pub fn name(&self) -> String {
        self.0.name()
    }
}

impl<T> CheckFrom for DumbID<T> where ObjectId<T> : Check<CheckedID<T>> {
    type Unchecked = DumbID<T, UncheckedIDs>;
    type Err = <ObjectId<T> as Check<CheckedID<T>>>::Err;

    fn check_from(us: Self::Unchecked) -> Result<Self, Self::Err> {
        Ok(Self(us.0.check()?))
    }
}