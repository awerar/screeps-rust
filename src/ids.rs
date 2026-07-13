use std::{fmt::Debug, hash::Hash, marker::PhantomData};

use derive_deref::{Deref, DerefMut};
use derive_where::derive_where;
use screeps::ObjectId;
use serde::{Deserialize, Serialize, Serializer};

use crate::{check::{Check, CheckFrom}, domain_traits::{HasId, IdReqs, MaybeResolvable}};

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

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct CheckedId<Id, S: CheckState = Checked> {
    id: Id,
    phantom: PhantomData<S>
}

impl<T: HasId> CheckedId<T::Id> {
    pub fn new(x: &T) -> Self {
        Self { id: x.id(), phantom: PhantomData }
    }
}

impl<T: screeps::MaybeHasId> CheckedId<T> {
    pub fn try_new(x: &T) -> Option<Self> {
        Some(Self { id: x.try_id()?, phantom: PhantomData })
    }
}

#[expect(unused)]
pub trait GetCheckedObjectId: screeps::HasId { 
    fn checked_id(&self) -> CheckedId<Self> {
        CheckedId::new(self)
    }
}

impl<T: screeps::HasId> GetCheckedObjectId for T { }

impl<T: screeps::HasId> CheckFrom for CheckedId<T> where ObjectId<T> : Check<T> {
    type Unchecked = CheckedId<T, Unchecked>;
    type Err = <ObjectId<T> as Check<T>>::Err;

    fn check_from(us: Self::Unchecked) -> Result<Self, Self::Err> {
        Ok(Self::new(&us.id.check()?))
    }
}