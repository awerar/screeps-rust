use std::{collections::{HashMap, HashSet}, fmt::Debug, hash::Hash, ops::Deref, rc::Rc};

use derive_where::derive_where;
use screeps::{ConstructionSite, Creep, HasId, MaybeHasId, ObjectId, SharedCreepProperties, game};
use serde::{Deserialize, Deserializer, Serialize, de::DeserializeOwned};
use wasm_bindgen::JsCast;

// TODO: Rename to checked / unchecked

pub trait DO = DeserializeOwned;

pub trait IDKind {
    type ID<T>: Serialize + Debug + Clone + PartialEq + Eq + PartialOrd + Ord + Hash;
}

#[derive(Clone, Copy, Deserialize, Serialize, Hash, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct SafeIDs {}
impl IDKind for SafeIDs {
    type ID<T> = SafeID<T>;
}

#[derive(Deserialize, Serialize, Hash, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct UnsafeIDs {}
impl IDKind for UnsafeIDs {
    type ID<T> = ObjectId<T>;
}

pub type UnsafeID<T> = ObjectId<T>;
pub struct SafeID<T> {
    pub id: ObjectId<T>,
    inner: Rc<T>
}

impl<T> AsRef<T> for SafeID<T> {
    fn as_ref(&self) -> &T {
        &self.inner
    }
}

impl<T> Debug for SafeID<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.id.fmt(f)
    }
}

impl<T> Clone for SafeID<T> {
    fn clone(&self) -> Self {
        Self { id: self.id, inner: self.inner.clone() }
    }
}

impl<T> Deref for SafeID<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner
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
        Some(self.cmp(other))
    }
}

impl SafeID<Creep> {
    pub fn from_name(name: String) -> Option<SafeID<Creep>> {
        Self::from_creep(game::creeps().get(name)?)
    }

    pub fn from_creep(creep: Creep) -> Option<SafeID<Creep>> {
        Some(SafeID { id: creep.try_id()?, inner: Rc::new(creep) })
    }

    pub fn creeps() -> impl Iterator<Item = SafeID<Creep>> {
        game::creeps().values().filter_map(Self::from_creep)
    }
}

pub trait ToSafeID<T> { fn to_safe_id(self) -> Option<SafeID<T>>; }
impl<T: JsCast + MaybeHasId> ToSafeID<T> for ObjectId<T> {
    fn to_safe_id(self) -> Option<SafeID<T>> {
        self.resolve().map(|entity| SafeID { id: self, inner: Rc::new(entity) })
    }
}

auto trait HasIDEntity {}
impl !HasIDEntity for Creep {}
impl !HasIDEntity for ConstructionSite {}

pub trait GetSafeID: Sized { 
    fn safe_id(&self) -> SafeID<Self>;
    #[expect(unused)] fn dumb_id(&self) -> DumbID<Self>;
}

impl<T: Clone + HasId + HasIDEntity> GetSafeID for T {
    default fn safe_id(&self) -> SafeID<Self> {
        SafeID { id: self.id(), inner: Rc::new(self.clone()) }
    }
    
    fn dumb_id(&self) -> DumbID<Self> {
        DumbID::new(self.safe_id())
    }
}

pub trait TryGetSafeID: Sized { fn try_safe_id(&self) -> Option<SafeID<Self>>; }
impl<T: GetSafeID> TryGetSafeID for T {
    fn try_safe_id(&self) -> Option<SafeID<Self>> {
        Some(self.safe_id())
    }
}

impl TryGetSafeID for ConstructionSite {
    fn try_safe_id(&self) -> Option<SafeID<Self>> {
        self.try_id().map(|id| SafeID { id, inner: Rc::new(self.clone()) })
    }
}

impl TryGetSafeID for Creep {
    fn try_safe_id(&self) -> Option<SafeID<Self>> {
        self.try_id().map(|id| SafeID { id, inner: Rc::new(self.clone()) })
    }
}

pub trait TriviallySafe {}

pub trait FromUnsafe {
    type Unsafe;

    fn from_unsafe(us: Self::Unsafe) -> Self;
}

pub trait TryFromUnsafe: Sized {
    type Unsafe;
    
    fn try_from_unsafe(us: Self::Unsafe) -> Option<Self>;
}

impl<T: TriviallySafe> FromUnsafe for T {
    type Unsafe = Self;

    fn from_unsafe(us: Self::Unsafe) -> Self {
        us
    }
}

impl<T: FromUnsafe> TryFromUnsafe for T {
    type Unsafe = Self;

    fn try_from_unsafe(us: Self::Unsafe) -> Option<Self> {
        Some(us)
    }
}

impl<T: JsCast + MaybeHasId + TryGetSafeID> TryFromUnsafe for SafeID<T> {
    type Unsafe = ObjectId<T>;

    fn try_from_unsafe(us: Self::Unsafe) -> Option<Self> {
        us.resolve()?.try_safe_id()
    }
}

impl<T: TryFromUnsafe> FromUnsafe for Option<T> {
    type Unsafe = Option<T::Unsafe>;

    fn from_unsafe(us: Self::Unsafe) -> Self {
        us.and_then(TryMakeSafe::try_make_safe)
    }
}

impl<T: TryFromUnsafe> FromUnsafe for Vec<T> {
    type Unsafe = Vec<T::Unsafe>;

    fn from_unsafe(us: Self::Unsafe) -> Self {
        us.into_iter().filter_map(TryMakeSafe::try_make_safe).collect()
    }
}

impl<T: TryFromUnsafe + Eq + Hash> FromUnsafe for HashSet<T> {
    type Unsafe = HashSet<T::Unsafe>;

    fn from_unsafe(us: Self::Unsafe) -> Self {
        us.into_iter().filter_map(TryMakeSafe::try_make_safe).collect()
    }
}

impl<K: TryFromUnsafe, V: TryFromUnsafe> TryFromUnsafe for (K, V) {
    type Unsafe = (K::Unsafe, V::Unsafe);

    fn try_from_unsafe(us: Self::Unsafe) -> Option<Self> {
        Some((us.0.try_make_safe()?, us.1.try_make_safe()?))
    }
}

impl <K: TryFromUnsafe + Hash + Eq, V: TryFromUnsafe> FromUnsafe for HashMap<K, V> {
    type Unsafe = HashMap<K::Unsafe, V::Unsafe>;

    fn from_unsafe(us: Self::Unsafe) -> Self {
        us.into_iter().filter_map(TryMakeSafe::try_make_safe).collect()
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

#[derive(Serialize, Deserialize)]
#[derive_where(PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Clone)]
#[serde(bound(deserialize = "I::ID<T> : DO", serialize = ""))]
pub struct DumbID<T, I : IDKind = SafeIDs>(I::ID<T>);

impl<T> From<SafeID<T>> for DumbID<T> {
    fn from(id: SafeID<T>) -> Self {
        DumbID::new(id)
    }
}

impl<T> SafeID<T> {
    pub fn dumb_id(&self) -> DumbID<T> {
        self.clone().into()
    }
}

impl<T> DumbID<T> {
    pub fn new(id: SafeID<T>) -> Self {
        Self(id)
    }
}

impl DumbID<Creep> {
    pub fn name(&self) -> String {
        self.0.name()
    }
}

impl<T> TryFromUnsafe for DumbID<T> where UnsafeID<T> : TryMakeSafe<SafeID<T>> {
    type Unsafe = DumbID<T, UnsafeIDs>;

    fn try_from_unsafe(us: Self::Unsafe) -> Option<Self> {
        Some(Self(us.0.try_make_safe()?))
    }
}

pub fn deserialize_from_unsafe<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D : Deserializer<'de>,
    T: FromUnsafe,
    T::Unsafe : Deserialize<'de>
{
    let raw = T::Unsafe::deserialize(deserializer)?;
    Ok(raw.make_safe())
}