use std::{collections::{HashMap, HashSet}, hash::Hash};

use itertools::Itertools;
use serde::{Deserialize, Deserializer};

// ==== Check traits ====
pub trait TriviallyChecked {}
impl TriviallyChecked for String {}

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

// ==== Container implementations ====
pub trait FilterCheck: Iterator + Sized {
    fn filter_check<T, B>(self) -> (B, Vec<T::Err>)
    where
        T: CheckFrom<Unchecked = Self::Item>,
        B: FromIterator<T>;
}

impl<I: Iterator> FilterCheck for I {
    fn filter_check<T, B>(self) -> (B, Vec<T::Err>)
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

impl<T: CheckFrom> FilterCheckFrom for Vec<T> {
    type Unchecked = Vec<T::Unchecked>;
    type Err = T::Err;

    fn filter_check_from(us: Self::Unchecked) -> (Self, Vec<Self::Err>) {
        us.into_iter().filter_check()
    }
}

impl<T: CheckFrom + Eq + Hash> FilterCheckFrom for HashSet<T> {
    type Unchecked = HashSet<T::Unchecked>;
    type Err = T::Err;

    fn filter_check_from(us: Self::Unchecked) -> (Self, Vec<Self::Err>) {
        us.into_iter().filter_check()
    }
}

impl <K: CheckFrom + Hash + Eq, V: CheckFrom> FilterCheckFrom for HashMap<K, V> {
    type Unchecked = HashMap<K::Unchecked, V::Unchecked>;
    type Err = PairCheckError<K::Err, V::Err>;

    fn filter_check_from(us: Self::Unchecked) -> (Self, Vec<Self::Err>) {
        us.into_iter().filter_check()
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