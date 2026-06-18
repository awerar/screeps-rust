use std::{collections::{HashMap, HashSet}, hash::Hash};

use serde::{Deserialize, Deserializer, de::DeserializeOwned};

pub trait DO = DeserializeOwned;

// ==== Check traits ====

pub trait TriviallyChecked {}

pub trait FromUnchecked {
    type Unchecked;

    fn from_unchecked(uc: Self::Unchecked) -> Self;
}

pub trait Check<S> {
    fn check(self) -> S;
}

pub trait TryFromUnchecked: Sized {
    type Unchecked;
    
    fn try_from_unchecked(uc: Self::Unchecked) -> Option<Self>;
}

pub trait TryCheck<S> {
    fn try_check(self) -> Option<S>;
}

// ==== Implied implementations ====

impl<T: TriviallyChecked> FromUnchecked for T {
    type Unchecked = Self;

    fn from_unchecked(uc: Self::Unchecked) -> Self {
        uc
    }
}

impl<S: FromUnchecked> Check<S> for S::Unchecked {
    fn check(self) -> S {
        S::from_unchecked(self)
    }
}

impl<T: FromUnchecked> TryFromUnchecked for T {
    type Unchecked = Self;

    fn try_from_unchecked(uc: Self::Unchecked) -> Option<Self> {
        Some(uc)
    }
}

impl<S: TryFromUnchecked> TryCheck<S> for S::Unchecked {
    fn try_check(self) -> Option<S> {
        S::try_from_unchecked(self)
    }
}

// ==== Container implementations ====

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

pub fn deserialize_check<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D : Deserializer<'de>,
    T: FromUnchecked,
    T::Unchecked : Deserialize<'de>
{
    let raw = T::Unchecked::deserialize(deserializer)?;
    Ok(raw.check())
}