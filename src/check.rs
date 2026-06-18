use std::{collections::{HashMap, HashSet}, hash::Hash};

use itertools::Either;
use serde::{Deserialize, Deserializer, de::DeserializeOwned};

pub trait DO = DeserializeOwned;

// ==== Check traits ====
// This is the main trait
pub trait TryFromUnchecked: Sized {
    type Unchecked;
    type Err;
    
    fn try_from_unchecked(uc: Self::Unchecked) -> Result<Self, Self::Err>;
}

pub trait FromUnchecked {
    type Unchecked;

    fn from_unchecked(uc: Self::Unchecked) -> Self;
}

pub trait TryCheck<T> {
    type Err;
    
    fn try_check(self) -> Result<T, Self::Err>;
}

pub trait Check<S> {
    fn check(self) -> S;
}

// ==== Implied implementations ====
impl<T: TryFromUnchecked> TryCheck<T> for T::Unchecked {
    type Err = T::Err;

    fn try_check(self) -> Result<T, T::Err> {
        T::try_from_unchecked(self)
    }
}

impl<T: TryFromUnchecked<Err = !>> FromUnchecked for T {
    type Unchecked = T::Unchecked;

    fn from_unchecked(uc: Self::Unchecked) -> Self {
        match T::try_from_unchecked(uc) {
            Ok(val) => val,
        }
    }
}
    

impl<T: FromUnchecked> Check<T> for T::Unchecked {
    fn check(self) -> T {
        T::from_unchecked(self)
    }
}

// ==== Container implementations ====

impl<T: TryFromUnchecked> FromUnchecked for Option<T> {
    type Unchecked = Option<T::Unchecked>;

    fn from_unchecked(us: Self::Unchecked) -> Self {
        us.and_then(|x| x.try_check().ok())
    }
}

impl<T: TryFromUnchecked> FromUnchecked for Vec<T> {
    type Unchecked = Vec<T::Unchecked>;

    fn from_unchecked(us: Self::Unchecked) -> Self {
        us.into_iter().filter_map(|x| x.try_check().ok()).collect()
    }
}

impl<T: TryFromUnchecked + Eq + Hash> FromUnchecked for HashSet<T> {
    type Unchecked = HashSet<T::Unchecked>;

    fn from_unchecked(us: Self::Unchecked) -> Self {
        us.into_iter().filter_map(|x| x.try_check().ok()).collect()
    }
}

impl<K: TryFromUnchecked, V: TryFromUnchecked> TryFromUnchecked for (K, V) {
    type Unchecked = (K::Unchecked, V::Unchecked);
    type Err = Either<K::Err, V::Err>;

    fn try_from_unchecked(us: Self::Unchecked) -> Result<Self, Self::Err> {
        Ok((us.0.try_check().map_err(Either::Left)?, us.1.try_check().map_err(Either::Right)?))
    }
}

impl <K: TryFromUnchecked + Hash + Eq, V: TryFromUnchecked> FromUnchecked for HashMap<K, V> {
    type Unchecked = HashMap<K::Unchecked, V::Unchecked>;

    fn from_unchecked(us: Self::Unchecked) -> Self {
        us.into_iter().filter_map(|x| x.try_check().ok()).collect()
    }
}

#[macro_export]
macro_rules! trivially_check {
    ($($ty:ty),* $(,)?) => {
        $(
            impl $crate::check::TryFromUnchecked for $ty {
                type Unchecked = Self;
                type Err = !;

                fn try_from_unchecked(uc: Self::Unchecked) -> Result<Self, Self::Err> {
                    Ok(uc)
                }
            }
        )*
    };
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