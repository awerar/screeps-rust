use std::{collections::{HashMap, hash_map}, hash::Hash};

use derive_where::derive_where;
use log::warn;
use screeps::Creep;
use serde::de::DeserializeOwned;
use serde_json_any_key::any_key_map;

use crate::{check::{CheckFrom, Expiring, ExpiringCheckError, FilterCheck, FilterCheckFrom, PairCheckError}, domain_traits::HasName, ids::{CheckState, Checked, Handle, Unchecked, WithId}};

const TIMEOUT: u32 = 1; // TODO: Make generic

#[derive_where(Serialize; K, V, S, K: Hash + Eq + 'static)]
#[derive_where(Deserialize; K: Hash + Eq + DeserializeOwned + 'static, V: DeserializeOwned + 'static, S: DeserializeOwned)]
pub struct ExpiringMap<V = (), K = Handle<WithId<Creep>>, S: CheckState = Checked> {
    #[serde(with = "any_key_map")] 
    keys: HashMap<K, Expiring<V, TIMEOUT, S>>
}

impl<V, K> IntoIterator for ExpiringMap<V, K> {
    type Item = (K, V);
    type IntoIter = impl Iterator<Item = Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.keys.into_iter().map(|(c, cr)| (c, cr.inner))
    }
}

impl<V, K> ExpiringMap<V, K> {
    pub fn new() -> Self {
        Self { keys: HashMap::new() }
    }
}

impl<V, K> ExpiringMap<V, K> where K : Hash + Eq {    
    pub fn add(&mut self, key: K, data: V) -> Option<V> {
        self.keys.insert(key, Expiring::new(data)).map(|expiry| expiry.inner)
    }

    pub fn refresh(&mut self, key: K) -> Option<LiveHandle<'_, V, K>> {
        match self.keys.entry(key) {
            hash_map::Entry::Vacant(_) => None,
            hash_map::Entry::Occupied(mut entry) => {
                entry.get_mut().expiration.refresh();
                Some(LiveHandle(entry))
            },
        }
    }
}

impl<V, K> Default for ExpiringMap<V, K> {
    fn default() -> Self {
        Self::new()
    }
}

pub struct LiveHandle<'a, V = (), K = Handle<WithId<Creep>>>(hash_map::OccupiedEntry<'a, K, Expiring<V, TIMEOUT>>);

impl<V, K> LiveHandle<'_, V, K> {
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

pub enum ExpiringEntryCheckError<V: CheckFrom, K: CheckFrom> {
    Key(K::Err, V::Unchecked),
    Value(K, V::Err),
    Expired(K, V)
}

impl<V, K> FilterCheckFrom for ExpiringMap<V, K> 
where
    K: CheckFrom + Hash + Eq + HasName,
    V: CheckFrom
{
    type Unchecked = ExpiringMap<V::Unchecked, K::Unchecked, Unchecked>;
    type Err = ExpiringEntryCheckError<V, K>;
    
    fn filter_check_from(uc: Self::Unchecked) -> (Self, Vec<Self::Err>) {
        let (keys, errs): (HashMap<K, _>, _) = uc.keys.filter_check();
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

        (Self { keys }, errs)
    }
}