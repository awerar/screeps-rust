use std::{collections::{HashMap, hash_map}, hash::Hash};

use derive_where::derive_where;
use log::warn;
use screeps::Creep;
use serde::de::DeserializeOwned;
use serde_json_any_key::any_key_map;

use crate::{check::{CheckFrom, Expiring, ExpiringCheckError, FilterCheck, FilterCheckFrom, PairCheckError}, domain_traits::HasName, ids::{CheckState, Checked, Handle, Unchecked, WithId}};

#[derive_where(Serialize; K, V, S, K: Hash + Eq + 'static)]
#[derive_where(Deserialize; K: Hash + Eq + DeserializeOwned + 'static, V: DeserializeOwned + 'static, S: DeserializeOwned)]
pub struct ExpiringMap<K, V, const TIMEOUT: u32 = 1, S: CheckState = Checked> {
    #[serde(with = "any_key_map")] 
    entries: HashMap<K, Expiring<V, TIMEOUT, S>>
}

pub type ExpiringCreepMap<V, const TIMEOUT: u32 = 1, S = Checked> = ExpiringMap<Handle<WithId<Creep>>, V, TIMEOUT, S>;

impl<K, V, const T: u32> IntoIterator for ExpiringMap<K, V, T> {
    type Item = (K, V);
    type IntoIter = impl Iterator<Item = Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.into_iter().map(|(c, cr)| (c, cr.inner))
    }
}

impl<K, V, const T: u32> ExpiringMap<K, V, T> {
    pub fn new() -> Self {
        Self { entries: HashMap::new() }
    }
}

impl<K, V, const T: u32> ExpiringMap<K, V, T> where K : Hash + Eq {    
    pub fn insert(&mut self, key: K, data: V) -> Option<V> {
        self.entries.insert(key, Expiring::new(data)).map(|expiry| expiry.inner)
    }

    pub fn refresh(&mut self, key: K) -> Option<LiveHandle<'_, K, V, T>> {
        match self.entries.entry(key) {
            hash_map::Entry::Vacant(_) => None,
            hash_map::Entry::Occupied(mut entry) => {
                entry.get_mut().expiration.refresh();
                Some(LiveHandle(entry))
            },
        }
    }
}

impl<K, V, const T: u32> Default for ExpiringMap<K, V, T> {
    fn default() -> Self {
        Self::new()
    }
}

pub struct LiveHandle<'a, K, V, const TIMEOUT: u32 = 1>(hash_map::OccupiedEntry<'a, K, Expiring<V, TIMEOUT>>);
pub type LiveCreepHandle<'a, V, const TIMEOUT: u32 = 1> = LiveHandle<'a, Handle<WithId<Creep>>, V, TIMEOUT>;

impl<K, V, const T: u32> LiveHandle<'_, K, V, T> {
    pub fn get(&self) -> &V {
        self.0.get()
    }

    pub fn get_mut(&mut self) -> &mut V {
        self.0.get_mut()
    }

    pub fn remove(self) {
        self.0.remove();
    }
}

pub enum ExpiringEntryCheckError<K: CheckFrom, V: CheckFrom> {
    Key(K::Err, V::Unchecked),
    Value(K, V::Err),
    Expired(K, V)
}

impl<K, V, const T: u32> FilterCheckFrom for ExpiringMap<K, V, T> 
where
    K: CheckFrom + Hash + Eq + HasName,
    V: CheckFrom
{
    type Unchecked = ExpiringMap<K::Unchecked, V::Unchecked, T, Unchecked>;
    type Err = ExpiringEntryCheckError<K, V>;
    
    fn filter_check_from(uc: Self::Unchecked) -> (Self, Vec<Self::Err>) {
        let (keys, errs): (HashMap<K, _>, _) = uc.entries.filter_check();
        for err in &errs {
            if let PairCheckError::Value(key, ExpiringCheckError::Expired(_)) = &err {
                warn!("{} timed out", key.name());
            }
        }

        let errs = errs.into_iter().map(|err| {
            match err {
                PairCheckError::Key(key_err, expiring_val) => {
                    let expiring_value: Expiring<_, _, _> = expiring_val; 
                    ExpiringEntryCheckError::Key(key_err, expiring_value.inner)
                },
                PairCheckError::Value(key, ExpiringCheckError::Inner(data_err)) => 
                    ExpiringEntryCheckError::Value(key, data_err),
                PairCheckError::Value(key, ExpiringCheckError::Expired(data)) =>
                    ExpiringEntryCheckError::Expired(key, data)
            }
        }).collect();

        (Self { entries: keys }, errs)
    }
}