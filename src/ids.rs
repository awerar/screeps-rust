use std::{fmt::Debug, hash::Hash, ops::Deref};

use derive_deref::{Deref, DerefMut};
use derive_where::derive_where;
use screeps::{Creep, ObjectId, game};
use serde::{Deserialize, Serialize, Serializer};
use wasm_bindgen::JsCast;

use crate::{check::{Check, CheckFrom}, domain_traits::{HasId, HasName, IdReqs, MaybeHasId, screeps_objects::IdResolutionError}};

pub trait CheckState: 'static {
    type Repr<T: HasId>: Serialize + Hash + Eq + Ord + Debug;
}

#[derive(Clone, Copy, Serialize, Hash, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Checked {}
impl CheckState for Checked {
    type Repr<T: HasId> = ById<T>;
}

#[derive(Clone, Copy, Deserialize, Serialize, Hash, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Unchecked {}
impl CheckState for Unchecked {
    type Repr<T: HasId> = T::Id;
}

#[derive(Deref, DerefMut)]
pub struct ById<T: HasId>(pub T);

impl<T: HasId> Serialize for ById<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.id().serialize(serializer)
    }
}

impl<T: HasId + Clone> Clone for ById<T> {
    fn clone(&self) -> Self {
        ById(self.0.clone())
    }
}

impl<T: HasId> Hash for ById<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.id().hash(state);
    }
}

impl<T: HasId> Eq for ById<T> {}
impl<T: HasId> PartialEq for ById<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0.id() == other.0.id()
    }
}

impl<T: HasId> PartialOrd for ById<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<T: HasId> Ord for ById<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.id().cmp(&other.id())
    }
}

impl<T: HasId> Debug for ById<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.id().fmt(f)
    }
}

impl<T: HasId + CheckFrom<Unchecked = T::Id>> CheckFrom for ById<T> {
    type Unchecked = T::Id;
    type Err = T::Err;

    fn check_from(uc: Self::Unchecked) -> Result<Self, Self::Err> {
        uc.check().map(ById)
    }
}

#[derive_where(Clone; Id: Clone, T: Clone)]
#[derive_where(Debug, PartialEq, Eq, Hash, Ord, PartialOrd; Id)]
pub struct WithId<T, Id = ObjectId<T>> {
    id: Id,
    #[derive_where(skip)] inner: T
}

impl<T, Id: IdReqs> HasId for WithId<T, Id> {
    type Id = Id;

    fn id(&self) -> Self::Id {
        self.id
    }
}

impl<T: MaybeHasId> WithId<T, T::Id> {
    pub fn new(entity: T) -> Option<Self> {
        Some(WithId { id: entity.try_id()?, inner: entity })
    }
}

impl<T, Id> AsRef<T> for WithId<T, Id> {
    fn as_ref(&self) -> &T {
        &self.inner
    }
}

impl<T, Id> Deref for WithId<T, Id> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T, Id: Serialize> Serialize for WithId<T, Id> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.id.serialize(serializer)
    }
}

impl<T: HasName, Id> HasName for WithId<T, Id> {
    fn name(&self) -> String {
        self.inner.name()
    }
}

impl WithId<Creep> {
    pub fn creeps() -> impl Iterator<Item = WithId<Creep>> {
        game::creeps().values().filter_map(Self::new)
    }
}

pub trait IntoWithId<O>: Sized { fn with_id(self) -> Option<WithId<Self, O>>;}
impl<T: MaybeHasId> IntoWithId<T::Id> for T {
    fn with_id(self) -> Option<WithId<Self, T::Id>> {
        WithId::new(self)
    }
}

impl<T: JsCast + screeps::MaybeHasId> CheckFrom for WithId<T> {
    type Unchecked = ObjectId<T>;
    type Err = IdResolutionError<T>;
    
    fn check_from(uc: Self::Unchecked) -> Result<Self, Self::Err> {
        uc.resolve().map(|entity| WithId { id: uc, inner: entity }).ok_or(IdResolutionError(uc))
    }
}

#[derive_where(Serialize, Deserialize; S::Repr<T>)]
#[derive_where(Clone; S::Repr<T>)]
#[derive_where(PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[serde(transparent)]
pub struct Handle<T: HasId, S: CheckState = Checked>(S::Repr<T>);

impl<T: HasId> Handle<T> {
    pub fn new(x: T) -> Self {
        Self(ById(x))
    }
}

#[expect(unused)]
pub trait IntoHandle: HasId { fn handle(self) -> Handle<Self>; }
impl<T: HasId> IntoHandle for T {
    fn handle(self) -> Handle<Self> {
        Handle::new(self)
    }
}

impl<T: HasId + HasName> HasName for Handle<T> {
    fn name(&self) -> String {
        self.0.name()
    }
}

impl<T: HasId> CheckFrom for Handle<T> where T::Id : Check<T> {
    type Unchecked = Handle<T, Unchecked>;
    type Err = <T::Id as Check<T>>::Err;

    fn check_from(us: Self::Unchecked) -> Result<Self, Self::Err> {
        Ok(Self::new(us.0.check()?))
    }
}