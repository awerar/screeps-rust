use std::{collections::{HashMap, HashSet}, hash::Hash};

use serde::{Deserialize, Deserializer, de::DeserializeOwned};

pub trait DO = DeserializeOwned;

// ==== Check traits ====
pub trait TriviallyChecked {}

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

// ==== Container implementations ====
pub trait FilterCheckFrom {
    type Unchecked;

    fn filter_check_from(uc: Self::Unchecked) -> Self;
}

pub trait FilterCheck<T> {
    fn filter_check(self) -> T;
}

impl<T: FilterCheckFrom> FilterCheck<T> for T::Unchecked {
    fn filter_check(self) -> T {
        T::filter_check_from(self)
    }
}

impl<T: CheckFrom> FilterCheckFrom for Option<T> {
    type Unchecked = Option<T::Unchecked>;

    fn filter_check_from(us: Self::Unchecked) -> Self {
        us.and_then(|x| x.check().ok())
    }
}

impl<T: CheckFrom> FilterCheckFrom for Vec<T> {
    type Unchecked = Vec<T::Unchecked>;

    fn filter_check_from(us: Self::Unchecked) -> Self {
        us.into_iter().filter_map(|x| x.check().ok()).collect()
    }
}

impl<T: CheckFrom + Eq + Hash> FilterCheckFrom for HashSet<T> {
    type Unchecked = HashSet<T::Unchecked>;

    fn filter_check_from(us: Self::Unchecked) -> Self {
        us.into_iter().filter_map(|x| x.check().ok()).collect()
    }
}

pub enum PairCheckError<KE, VE> {
    Key(KE),
    Value(VE)
}

impl<K: CheckFrom, V: CheckFrom> CheckFrom for (K, V) {
    type Unchecked = (K::Unchecked, V::Unchecked);
    type Err = PairCheckError<K::Err, V::Err>;

    fn check_from(us: Self::Unchecked) -> Result<Self, Self::Err> {
        Ok((us.0.check().map_err(PairCheckError::Key)?, us.1.check().map_err(PairCheckError::Value)?))
    }
}

impl <K: CheckFrom + Hash + Eq, V: CheckFrom> FilterCheckFrom for HashMap<K, V> {
    type Unchecked = HashMap<K::Unchecked, V::Unchecked>;

    fn filter_check_from(us: Self::Unchecked) -> Self {
        us.into_iter().filter_map(|x| x.check().ok()).collect()
    }
}

pub fn deserialize_filter_check<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D : Deserializer<'de>,
    T: FilterCheckFrom,
    T::Unchecked : Deserialize<'de>
{
    let raw = T::Unchecked::deserialize(deserializer)?;
    Ok(raw.filter_check())
}