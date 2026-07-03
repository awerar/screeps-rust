use std::{fmt::Debug, hash::Hash};

use derive_where::derive_where;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{check::{CheckFrom, FilterCheck, FilterCheckFrom, TriviallyChecked}, new_tasks::{client_registry::{ClientDataCheckError, ClientEntryCheckError, ClientHandle, ClientRegistry}, server::UpdateableTaskData}};

#[derive(Serialize, Deserialize)]
pub struct ClientData {
    pending_work: u32
}

impl TriviallyChecked for ClientData {}

#[derive(Serialize, Deserialize)]
pub struct TaskData {
    remaining_work: u32,
    pending_work: u32
}

#[derive_where(Serialize, Deserialize; ClientRegistry<Client, ClientData>)]
pub struct CollaborativeClientRegistry<Client> {
    registry: ClientRegistry<Client, ClientData>,
    task_data: TaskData
}

impl<C> CollaborativeClientRegistry<C> {
    pub fn new(required_work: u32) -> Self {
        Self { 
            registry: ClientRegistry::new(),
            task_data: TaskData { 
                remaining_work: required_work, 
                pending_work: 0
            }
        }
    }
}

impl<Client: Hash + Eq> CollaborativeClientRegistry<Client> {
    pub fn heartbeat(&mut self, client: Client) -> Option<CollaborativeClientHandle<'_, Client>> {
        Some(CollaborativeClientHandle {
            client_handle: self.registry.heartbeat(client)?,
            task_data: &mut self.task_data
        })
    }

    pub fn add(&mut self, client: Client, work: u32) {
        self.registry.add(client, ClientData { pending_work: work });
        self.task_data.pending_work += work;
    }
}

pub struct RemainingWork(pub u32);
impl<Client> UpdateableTaskData for CollaborativeClientRegistry<Client> {
    type Update = RemainingWork;

    fn update(&mut self, update: Self::Update) {
        self.task_data.remaining_work = update.0;
    }

    fn create(update: Self::Update) -> Self {
        Self::new(update.0)
    }
}

#[derive(Error, Debug)]
pub enum ClientCheckError<Client: CheckFrom> {
    #[error("Timed out")] Timeout(Client),
    #[error("Client check failed: {0}")] ClientCheck(Client::Err)
}

impl<Client: CheckFrom + Hash + Eq + Debug> FilterCheckFrom for CollaborativeClientRegistry<Client> {
    type Unchecked = CollaborativeClientRegistry<Client::Unchecked>;
    type Err = ClientCheckError<Client>;

    fn filter_check_from(uc: Self::Unchecked) -> (Self, Vec<Self::Err>) {
        let (registry, errs) = uc.registry.filter_check();

        let mut checked = Self { 
            registry,
            task_data: uc.task_data
        };

        let mut new_errs = Vec::new();
        for err in errs {
            let (client_data, new_err) = match err {
                ClientEntryCheckError::Client(client_err, client_data) => {
                    (client_data, ClientCheckError::ClientCheck(client_err))
                },
                ClientEntryCheckError::Data(client, ClientDataCheckError::Timeout(client_data)) => 
                    (client_data, ClientCheckError::Timeout(client)),
            };

            checked.task_data.pending_work -= client_data.pending_work;
            new_errs.push(new_err);
        }

        (checked, new_errs)
    }
}

pub struct CollaborativeClientHandle<'a, Client> {
    task_data: &'a mut TaskData,
    client_handle: ClientHandle<'a, Client, ClientData>
}

impl<Client> CollaborativeClientHandle<'_, Client> {
    pub fn apply_work(&mut self, amount: u32) {
        self.task_data.pending_work = self.task_data.pending_work.saturating_sub(amount);
        self.task_data.remaining_work = self.task_data.remaining_work.saturating_sub(amount);
        self.client_handle.get_mut().pending_work = self.client_handle.get().pending_work.saturating_sub(amount);
    }

    pub fn remove(self) {
        self.task_data.pending_work += self.client_handle.get().pending_work;
        self.client_handle.remove();
    }

    pub fn remaining(&self) -> u32 {
        self.client_handle.get().pending_work
    }
}