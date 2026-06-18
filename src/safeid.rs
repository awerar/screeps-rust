use std::{collections::{HashMap, HashSet}, fmt::Debug, hash::Hash, ops::Deref, rc::Rc};

use derive_where::derive_where;
use screeps::{ConstructionSite, Creep, HasId, MaybeHasId, ObjectId, SharedCreepProperties, game};
use serde::{Deserialize, Deserializer, Serialize, de::DeserializeOwned};
use wasm_bindgen::JsCast;

pub trait DO = DeserializeOwned;

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

pub type UncheckedID<T> = ObjectId<T>;
pub struct CheckedID<T> {
    pub id: ObjectId<T>,
    inner: Rc<T>
}

impl<T> AsRef<T> for CheckedID<T> {
    fn as_ref(&self) -> &T {
        &self.inner
    }
}

impl<T> Debug for CheckedID<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.id.fmt(f)
    }
}

impl<T> Clone for CheckedID<T> {
    fn clone(&self) -> Self {
        Self { id: self.id, inner: self.inner.clone() }
    }
}

impl<T> Deref for CheckedID<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T> Serialize for CheckedID<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: serde::Serializer {
        self.id.serialize(serializer)
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
        Some(self.cmp(other))
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

pub trait ToCheckedID<T> { fn check_id(self) -> Option<CheckedID<T>>; }
impl<T: JsCast + MaybeHasId> ToCheckedID<T> for ObjectId<T> {
    fn check_id(self) -> Option<CheckedID<T>> {
        self.resolve().map(|entity| CheckedID { id: self, inner: Rc::new(entity) })
    }
}

auto trait HasIDEntity {}
impl !HasIDEntity for Creep {}
impl !HasIDEntity for ConstructionSite {}

pub trait GetCheckedID: Sized { 
    fn check_id(&self) -> CheckedID<Self>;
    #[expect(unused)] fn dumb_id(&self) -> DumbID<Self>;
}

impl<T: Clone + HasId + HasIDEntity> GetCheckedID for T {
    default fn check_id(&self) -> CheckedID<Self> {
        CheckedID { id: self.id(), inner: Rc::new(self.clone()) }
    }
    
    fn dumb_id(&self) -> DumbID<Self> {
        DumbID::new(self.check_id())
    }
}

pub trait TryGetCheckedID: Sized { fn try_check_id(&self) -> Option<CheckedID<Self>>; }
impl<T: GetCheckedID> TryGetCheckedID for T {
    fn try_check_id(&self) -> Option<CheckedID<Self>> {
        Some(self.check_id())
    }
}

impl TryGetCheckedID for ConstructionSite {
    fn try_check_id(&self) -> Option<CheckedID<Self>> {
        self.try_id().map(|id| CheckedID { id, inner: Rc::new(self.clone()) })
    }
}

impl TryGetCheckedID for Creep {
    fn try_check_id(&self) -> Option<CheckedID<Self>> {
        self.try_id().map(|id| CheckedID { id, inner: Rc::new(self.clone()) })
    }
}

pub trait TriviallyChecked {}

pub trait FromUnchecked {
    type Unchecked;

    fn from_unchecked(uc: Self::Unchecked) -> Self;
}

pub trait TryFromUnchecked: Sized {
    type Unchecked;
    
    fn try_from_unchecked(uc: Self::Unchecked) -> Option<Self>;
}

impl<T: TriviallyChecked> FromUnchecked for T {
    type Unchecked = Self;

    fn from_unchecked(uc: Self::Unchecked) -> Self {
        uc
    }
}

impl<T: FromUnchecked> TryFromUnchecked for T {
    type Unchecked = Self;

    fn try_from_unchecked(uc: Self::Unchecked) -> Option<Self> {
        Some(uc)
    }
}

impl<T: JsCast + MaybeHasId + TryGetCheckedID> TryFromUnchecked for CheckedID<T> {
    type Unchecked = ObjectId<T>;

    fn try_from_unchecked(us: Self::Unchecked) -> Option<Self> {
        us.resolve()?.try_check_id()
    }
}

impl<T: TryFromUnchecked> FromUnchecked for Option<T> {
    type Unchecked = Option<T::Unchecked>;

    fn from_unchecked(us: Self::Unchecked) -> Self {
        us.and_then(TryCheck::try_check)
    }
}

impl<T: TryFromUnchecked> FromUnchecked for Vec<T> {
    type Unchecked = Vec<T::Unchecked>;

    fn from_unchecked(us: Self::Unchecked) -> Self {
        us.into_iter().filter_map(TryCheck::try_check).collect()
    }
}

impl<T: TryFromUnchecked + Eq + Hash> FromUnchecked for HashSet<T> {
    type Unchecked = HashSet<T::Unchecked>;

    fn from_unchecked(us: Self::Unchecked) -> Self {
        us.into_iter().filter_map(TryCheck::try_check).collect()
    }
}

impl<K: TryFromUnchecked, V: TryFromUnchecked> TryFromUnchecked for (K, V) {
    type Unchecked = (K::Unchecked, V::Unchecked);

    fn try_from_unchecked(us: Self::Unchecked) -> Option<Self> {
        Some((us.0.try_check()?, us.1.try_check()?))
    }
}

impl <K: TryFromUnchecked + Hash + Eq, V: TryFromUnchecked> FromUnchecked for HashMap<K, V> {
    type Unchecked = HashMap<K::Unchecked, V::Unchecked>;

    fn from_unchecked(us: Self::Unchecked) -> Self {
        us.into_iter().filter_map(TryCheck::try_check).collect()
    }
}

pub trait Check<S> {
    fn check(self) -> S;
}

impl<S: FromUnchecked> Check<S> for S::Unchecked {
    fn check(self) -> S {
        S::from_unchecked(self)
    }
}

pub trait TryCheck<S> {
    fn try_check(self) -> Option<S>;
}

impl<S: TryFromUnchecked> TryCheck<S> for S::Unchecked {
    fn try_check(self) -> Option<S> {
        S::try_from_unchecked(self)
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

impl<T> TryFromUnchecked for DumbID<T> where UncheckedID<T> : TryCheck<CheckedID<T>> {
    type Unchecked = DumbID<T, UncheckedIDs>;

    fn try_from_unchecked(us: Self::Unchecked) -> Option<Self> {
        Some(Self(us.0.try_check()?))
    }
}

pub fn deserialize_check<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D : Deserializer<'de>,
    T: FromUnchecked,
    T::Unchecked : Deserialize<'de>
{
    let raw = T::Unchecked::deserialize(deserializer)?;
    Ok(raw.check())
}