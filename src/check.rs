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