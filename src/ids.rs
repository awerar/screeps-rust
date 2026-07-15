use std::{fmt::Debug, hash::Hash};

use derive_where::derive_where;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::{check::{Check, CheckFrom}, domain_traits::HasId};

pub trait CheckState: 'static { }

#[derive(Clone, Copy, Serialize, Hash, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Checked {}
impl CheckState for Checked { }

#[derive(Clone, Copy, Deserialize, Serialize, Hash, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Unchecked {}
impl CheckState for Unchecked { }

#[derive_where(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash, Debug; T::Id<S>)]
pub struct Handle<T: HasId, S: CheckState = Checked>(T::Id<S>);

impl<T: HasId> Handle<T> {
    pub fn new(x: &T) -> Self {
        Handle(x.id())
    }

    pub fn from_id(id: T::Id<Checked>) -> Self {
        Handle(id)
    }
}

impl<T: HasId> GetHandle for T {}
pub trait GetHandle: HasId {
    fn handle(&self) -> Handle<Self> {
        Handle(self.id())
    }
}

impl<T: HasId> CheckFrom for Handle<T> 
where 
    T::Id<Checked> : CheckFrom<Unchecked = T::Id<Unchecked>>,
    T::Id<Unchecked> : DeserializeOwned
{
    type Unchecked = Handle<T, Unchecked>;
    type Err = <T::Id<Checked> as CheckFrom>::Err;

    fn check_from(uc: Self::Unchecked) -> Result<Self, Self::Err> {
        Ok(Handle(uc.0.check()?))
    }
}