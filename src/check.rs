use std::{collections::{HashMap, HashSet}, hash::Hash, ops::{Deref, DerefMut}};

use derive_deref::{Deref, DerefMut};
use derive_where::derive_where;
use itertools::Itertools;
use screeps::{Position, game};
use serde::{Deserialize, Deserializer, Serialize};

// ==== Check traits ====
pub trait TriviallyChecked {}
impl TriviallyChecked for String {}
impl TriviallyChecked for u32 {}
impl TriviallyChecked for Position {}
impl TriviallyChecked for () {}

pub trait CheckFrom: Sized {
    type Unchecked;
    type Err;
    
    fn check_from(uc: Self::Unchecked) -> Result<Self, Self::Err>;
}

pub trait Check<T> {
    type Err;
    
    fn check(self) -> Result<T, Self::Err>;
}

// ==== Implied implementations ====
impl<T: TriviallyChecked> CheckFrom for T {
    type Unchecked = Self;
    type Err = !;

    fn check_from(uc: Self::Unchecked) -> Result<Self, Self::Err> {
        Ok(uc)
    }
}

impl<T: CheckFrom> Check<T> for T::Unchecked {
    type Err = T::Err;

    fn check(self) -> Result<T, T::Err> {
        T::check_from(self)
    }
}

// Pair check, used for hashmap filter checks
pub enum PairCheckError<K: CheckFrom, V: CheckFrom> {
    Key(K::Err, V::Unchecked),
    Value(K, V::Err)
}

impl<K: CheckFrom, V: CheckFrom> CheckFrom for (K, V) {
    type Unchecked = (K::Unchecked, V::Unchecked);
    type Err = PairCheckError<K, V>;

    fn check_from(us: Self::Unchecked) -> Result<Self, Self::Err> {
        let key = match us.0.check() {
            Ok(key) => key,
            Err(ke) => return Err(PairCheckError::Key(ke, us.1))
        };

        let value = match us.1.check() {
            Ok(value) => value,
            Err(ve) => return Err(PairCheckError::Value(key, ve))
        };

        Ok((key, value))
    }
}

// ==== Container implementations ====
impl<T: CheckFrom> CheckFrom for Option<T> {
    type Unchecked = Option<T::Unchecked>;
    type Err = T::Err;

    fn check_from(uc: Self::Unchecked) -> Result<Self, Self::Err> {
        match uc {
            Some(val) => Ok(Some(val.check()?)),
            None => Ok(None),
        }
    }
}

pub trait FilterCheckIterator: Iterator + Sized {
    fn filter_check_iter<T, B>(self) -> (B, Vec<T::Err>)
    where
        T: CheckFrom<Unchecked = Self::Item>,
        B: FromIterator<T>;
}

impl<I: Iterator> FilterCheckIterator for I {
    fn filter_check_iter<T, B>(self) -> (B, Vec<T::Err>)
    where
        T: CheckFrom<Unchecked = Self::Item>,
        B: FromIterator<T>,
    {
        let (values, errs): (Vec<_>, Vec<_>) = self.map(Check::check).partition_result(); 
        (values.into_iter().collect(), errs)
    }
}

pub trait FilterCheckFrom: Sized {
    type Unchecked;
    type Err;

    fn filter_check_from(uc: Self::Unchecked) -> (Self, Vec<Self::Err>);
}

pub trait FilterCheck<T> {
    type Err;
    
    fn filter_check(self) -> (T, Vec<Self::Err>);
}

impl<T: FilterCheckFrom> FilterCheck<T> for T::Unchecked {
    type Err = T::Err;

    fn filter_check(self) -> (T, Vec<Self::Err>) {
        T::filter_check_from(self)
    }
}

impl<T: CheckFrom> FilterCheckFrom for Vec<T> {
    type Unchecked = Vec<T::Unchecked>;
    type Err = T::Err;

    fn filter_check_from(us: Self::Unchecked) -> (Self, Vec<Self::Err>) {
        us.into_iter().filter_check_iter()
    }
}

impl<T: CheckFrom + Eq + Hash> FilterCheckFrom for HashSet<T> {
    type Unchecked = HashSet<T::Unchecked>;
    type Err = T::Err;

    fn filter_check_from(us: Self::Unchecked) -> (Self, Vec<Self::Err>) {
        us.into_iter().filter_check_iter()
    }
}

impl <K: CheckFrom + Hash + Eq, V: CheckFrom> FilterCheckFrom for HashMap<K, V> {
    type Unchecked = HashMap<K::Unchecked, V::Unchecked>;
    type Err = PairCheckError<K, V>;

    fn filter_check_from(us: Self::Unchecked) -> (Self, Vec<Self::Err>) {
        us.into_iter().filter_check_iter()
    }
}

pub fn deserialize_filter_check<'de, D, B>(deserializer: D) -> Result<B, D::Error>
where
    D : Deserializer<'de>,
    B: FilterCheckFrom,
    B::Unchecked : Deserialize<'de>
{
    let raw = B::Unchecked::deserialize(deserializer)?;
    Ok(B::filter_check_from(raw).0)
}

#[derive(Clone, Copy, Serialize, Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Deref, DerefMut)]
#[serde(transparent)]
pub struct Filtered<T>(pub T);
impl<T> TriviallyChecked for Filtered<T> {}

impl<'de, T: FilterCheckFrom> Deserialize<'de> for Filtered<T> where T::Unchecked : Deserialize<'de> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Filtered(deserialize_filter_check(deserializer)?))
    }
}

// ==== Expiration ====
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Expiration<const LIFETIME: u32> {
    last_refresh: u32
}

impl<const LIFETIME: u32> Expiration<LIFETIME> {
    pub fn new() -> Self {
        Self { last_refresh: game::time() }
    }

    pub fn refresh(&mut self) {
        self.last_refresh = game::time();
    }

    pub fn time_left(&self) -> u32 {
        (self.last_refresh + LIFETIME + 1).saturating_sub(game::time())
    }
}

impl<const LT: u32> CheckFrom for Expiration<LT> {
    type Unchecked = Self;
    type Err = ();

    fn check_from(uc: Self::Unchecked) -> Result<Self, Self::Err> {
        if uc.time_left() == 0 { return Err(()) }

        Ok(uc)
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[derive_where(PartialEq, Eq, PartialOrd, Ord; T)]
pub struct Expiring<T, const LIFETIME: u32> {
    pub inner: T,
    #[derive_where(skip)] 
    pub expiration: Expiration<LIFETIME>
}

impl<T, const LT: u32> Expiring<T, LT> {
    pub fn new(inner: T) -> Self {
        Expiring { inner, expiration: Expiration::new() }
    }
}

impl<T, const E: u32> Deref for Expiring<T, E> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T, const E: u32> DerefMut for Expiring<T, E> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

pub enum ExpirationCheckError<T: CheckFrom> {
    Expired(T),
    Inner(T::Err)
}

impl<T: CheckFrom, const EXPIRY: u32> CheckFrom for Expiring<T, EXPIRY> {
    type Unchecked = Expiring<T::Unchecked, EXPIRY>;
    type Err = ExpirationCheckError<T>;

    fn check_from(uc: Self::Unchecked) -> Result<Self, Self::Err> {
        let inner: T = uc.inner.check().map_err(ExpirationCheckError::Inner)?;
        let Ok(expiration) = uc.expiration.check() else {
            return Err(ExpirationCheckError::Expired(inner))
        };

        Ok(Self { inner, expiration })
    }
}