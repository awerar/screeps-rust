use std::{collections::{HashMap, hash_map}, hash::Hash};

use derive_where::derive_where;
use screeps::game;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::check::{Check, CheckFrom, FilterCheck, FilterCheckFrom};

const TIMEOUT: u32 = 2;

#[derive(Serialize, Deserialize)]
struct ClientEntry<ClientData> {
    last_heartbeat: u32,
    data: ClientData
}

#[derive(Error, Debug)]
pub enum ClientDataCheckError<CD: CheckFrom> {
    #[error("Timed out")] Timeout(CD),
    #[error("Data check failed: {0}")] DataCheck(CD::Err)
}

impl<CD: CheckFrom> CheckFrom for ClientEntry<CD> {
    type Unchecked = ClientEntry<CD::Unchecked>;
    type Err = ClientDataCheckError<CD>;

    fn check_from(uc: Self::Unchecked) -> Result<Self, Self::Err> {
        let data = uc.data.check().map_err(ClientDataCheckError::DataCheck)?;
        if game::time() > uc.last_heartbeat + TIMEOUT { return Err(ClientDataCheckError::Timeout(data)) }

        Ok(Self {
            data,
            .. uc
        })
    }
}

impl<CD> ClientEntry<CD> {
    pub fn new(data: CD) -> Self {
        Self { last_heartbeat: game::time(), data }
    }
}

#[derive_where(Serialize, Deserialize; HashMap<Client, ClientEntry<ClientData>>)]
pub struct ClientRegistry<Client, ClientData> {
    clients: HashMap<Client, ClientEntry<ClientData>>
}

impl<C, CD> IntoIterator for ClientRegistry<C, CD> {
    type Item = (C, CD);
    type IntoIter = impl Iterator<Item = Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.clients.into_iter().map(|(c, cr)| (c, cr.data))
    }
}

impl<C: Eq + Hash, CD> FromIterator<(C, CD)> for ClientRegistry<C, CD> {
    fn from_iter<T: IntoIterator<Item = (C, CD)>>(iter: T) -> Self {
        Self {
            clients: iter.into_iter()
                .map(|(c, cd)| (c, ClientEntry::new(cd)))
                .collect()
        }

    }
}

impl<Client, ClientData> ClientRegistry<Client, ClientData> where Client : Hash + Eq {
    pub fn new() -> Self {
        Self { clients: HashMap::new() }
    }
    
    pub fn add(&mut self, client: Client, data: ClientData) {
        self.clients.insert(client, ClientEntry::new(data));
    }

    pub fn heartbeat(&mut self, client: Client) -> Option<ClientHandle<'_, Client, ClientData>> {
        match self.clients.entry(client) {
            hash_map::Entry::Vacant(_) => None,
            hash_map::Entry::Occupied(mut entry) => {
                entry.get_mut().last_heartbeat = game::time();
                Some(ClientHandle(entry))
            },
        }
    }
}

pub struct ClientHandle<'a, Client, ClientData>(hash_map::OccupiedEntry<'a, Client, ClientEntry<ClientData>>);

impl<C, CD> ClientHandle<'_, C, CD> {
    pub fn get(&self) -> &CD {
        &self.0.get().data
    }

    pub fn get_mut(&mut self) -> &mut CD {
        &mut self.0.get_mut().data
    }

    pub fn remove(self) {
        self.0.remove();
    }
}

impl<C: CheckFrom + Hash + Eq, CD: CheckFrom> FilterCheckFrom for ClientRegistry<C, CD> {
    type Unchecked = ClientRegistry<C::Unchecked, CD::Unchecked>;
    type Err = <(C, CD) as CheckFrom>::Err;
    
    fn filter_check_from(uc: Self::Unchecked) -> (Self, Vec<Self::Err>) {
        uc.into_iter().filter_check()
    }
}