use std::{collections::HashMap, hash::Hash};

use derive_where::derive_where;
use itertools::Itertools;
use screeps::game;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::check::{Check, CheckFrom, FilterCheck, FilterCheckFrom};

#[derive(Serialize, Deserialize)]
struct ClientEntry<ClientData, const TIMEOUT: u32> {
    last_heartbeat: u32,
    data: ClientData
}

#[derive(Error, Debug)]
pub enum ClientDataCheckError<CD: CheckFrom> {
    #[error("Timed out")] Timeout(CD),
    #[error("Data check failed: {0}")] DataCheck(CD::Err)
}

impl<CD: CheckFrom, const TIMEOUT: u32> CheckFrom for ClientEntry<CD, TIMEOUT> {
    type Unchecked = ClientEntry<CD::Unchecked, TIMEOUT>;
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

impl<CD, const T: u32> ClientEntry<CD, T> {
    pub fn new(data: CD) -> Self {
        Self { last_heartbeat: game::time(), data }
    }
}

#[derive_where(Serialize, Deserialize; HashMap<Client, ClientEntry<ClientData, TIMEOUT>>)]
pub struct ClientRegistry<Client, ClientData, const TIMEOUT: u32 = 5> {
    clients: HashMap<Client, ClientEntry<ClientData, TIMEOUT>>
}

impl<C, CD, const T: u32> IntoIterator for ClientRegistry<C, CD, T> {
    type Item = (C, CD);
    type IntoIter = impl Iterator<Item = Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.clients.into_iter().map(|(c, cr)| (c, cr.data))
    }
}

impl<C: Eq + Hash, CD, const TI: u32> FromIterator<(C, CD)> for ClientRegistry<C, CD, TI> {
    fn from_iter<T: IntoIterator<Item = (C, CD)>>(iter: T) -> Self {
        Self {
            clients: iter.into_iter()
                .map(|(c, cd)| (c, ClientEntry::new(cd)))
                .collect()
        }

    }
}

impl<Client, ClientData, const TIMEOUT: u32> ClientRegistry<Client, ClientData, TIMEOUT> where Client : Hash + Eq {
    pub fn add(&mut self, client: Client, data: ClientData) {
        self.clients.insert(client, ClientEntry::new(data));
    }

    pub fn remove(&mut self, client: &Client) -> Option<ClientData> {
        self.clients.remove(client).map(|x| x.data)
    }

    pub fn get(&self, client: &Client) -> Option<&ClientData> {
        self.clients.get(client).map(|x| &x.data)
    }

    pub fn get_mut(&mut self, client: &mut Client) -> Option<&mut ClientData> {
        self.clients.get_mut(client).map(|x| &mut x.data)
    }

    pub fn heartbeat(&mut self, client: &Client) -> bool {
        let Some(client_record) = self.clients.get_mut(client) else { return false };

        client_record.last_heartbeat = game::time();
        true
    }
}

impl<C: CheckFrom + Hash + Eq, CD: CheckFrom, const T: u32> FilterCheckFrom for ClientRegistry<C, CD, T> {
    type Unchecked = ClientRegistry<C::Unchecked, CD::Unchecked>;
    type Err = <(C, CD) as CheckFrom>::Err;
    
    fn filter_check_from(uc: Self::Unchecked) -> (Self, Vec<Self::Err>) {
        uc.into_iter().filter_check()
    }
}