use std::{collections::{HashMap, hash_map}, fmt::Debug, hash::Hash};

use derive_where::derive_where;
use log::warn;
use screeps::game;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use thiserror::Error;
use serde_json_any_key::any_key_map;

use crate::check::{Check, CheckFrom, FilterCheck, FilterCheckFrom, PairCheckError};

const TIMEOUT: u32 = 2;

#[derive(Serialize, Deserialize)]
struct ClientEntry<ClientData> {
    last_heartbeat: u32,
    data: ClientData
}

#[derive(Error, Debug)]
pub enum ClientDataCheckError<ClientData: CheckFrom> {
    #[error("Timed out")] Timeout(ClientData),
    #[error("Data check failed: {0}")] DataCheck(ClientData::Err)
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

#[derive_where(Serialize; Client, ClientData, Client: Hash + Eq + 'static)]
#[derive_where(Deserialize; Client: Hash + Eq + DeserializeOwned + 'static, ClientData: DeserializeOwned + 'static)]
pub struct ClientRegistry<Client, ClientData> {
    #[serde(with = "any_key_map")] 
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

#[derive(Error)]
#[derive_where(Debug; Client, ClientData, Client::Err, ClientData::Err, ClientData::Unchecked)]
pub enum ClientEntryCheckError<Client: CheckFrom, ClientData: CheckFrom> {
    #[error("Client check failed: {0}")] Client(Client::Err, ClientData::Unchecked),
    #[error("{1}")] Data(Client, ClientDataCheckError<ClientData>),
}

impl<Client, ClientData> FilterCheckFrom for ClientRegistry<Client, ClientData> 
where
    Client: CheckFrom + Hash + Eq + Debug,
    ClientData: CheckFrom
{
    type Unchecked = ClientRegistry<Client::Unchecked, ClientData::Unchecked>;
    type Err = ClientEntryCheckError<Client, ClientData>;
    
    fn filter_check_from(uc: Self::Unchecked) -> (Self, Vec<Self::Err>) {
        let (clients, errs) = uc.clients.filter_check();
        for err in &errs {
            if let PairCheckError::Value(client, ClientDataCheckError::Timeout(_)) = &err {
                warn!("{client:?} timed out");
            }
        }

        let errs = errs.into_iter().map(|err| {
            match err {
                PairCheckError::Key(client_error, client_entry) => {
                    let client_entry: ClientEntry<ClientData::Unchecked> = client_entry; 
                    ClientEntryCheckError::Client(client_error, client_entry.data)
                },
                PairCheckError::Value(client, client_data_error) => 
                    ClientEntryCheckError::Data(client, client_data_error)
            }
        }).collect();

        (Self { clients }, errs)
    }
}