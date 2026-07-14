use std::{fmt::Debug, hash::Hash};

use derive_deref::{Deref, DerefMut};
use serde::{Deserialize, Serialize, Serializer};

use crate::{check::{Check, CheckFrom}, domain_traits::{HasId, ResolvableId}};

pub trait CheckState: 'static {
    type Repr<T: HasId>;
}

#[derive(Clone, Copy, Serialize, Hash, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Checked {}
impl CheckState for Checked {
    type Repr<T: HasId> = ById<T>;
}

#[derive(Clone, Copy, Deserialize, Serialize, Hash, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Unchecked {}
impl CheckState for Unchecked {
    type Repr<T: HasId> = <T::Id as CheckFrom>::Unchecked;
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

impl<T: HasId> CheckFrom for ById<T>
where
    T::Id : CheckFrom + ResolvableId<Target = T>
{
    type Unchecked = <T::Id as CheckFrom>::Unchecked;
    type Err = <T::Id as CheckFrom>::Err;

    fn check_from(uc: Self::Unchecked) -> Result<Self, Self::Err> {
        let id: T::Id = uc.check()?; 
        Ok(ById(id.resolve()))
    }
}