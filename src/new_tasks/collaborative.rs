use std::{hash::Hash, marker::PhantomData};

use derive_where::derive_where;
use serde::{Deserialize, Serialize};

use crate::{check::{CheckFrom, FilterCheck, FilterCheckFrom, PairCheckError, TriviallyChecked}, ids::{CheckState, Checked}, new_tasks::client_registry::{ClientHandle, ClientRegistry}};

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

#[derive_where(Serialize, Deserialize; ClientRegistry<Client, ClientData>, S)]
pub struct CollaborativeClientRegistry<Client, S: CheckState = Checked> {
    registry: ClientRegistry<Client, ClientData>,
    task_data: TaskData,
    phantom: PhantomData<S>
}

impl<Client: Hash + Eq> CollaborativeClientRegistry<Client> {
    pub fn new(required_work: u32) -> Self {
        Self { 
            registry: ClientRegistry::new(),
            task_data: TaskData { 
                remaining_work: required_work, 
                pending_work: 0
            },
            phantom: PhantomData
        }
    }

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

impl<Client: CheckFrom + Hash + Eq> FilterCheckFrom for CollaborativeClientRegistry<Client> {
    type Unchecked = CollaborativeClientRegistry<Client::Unchecked>;
    type Err = Client::Err;

    fn filter_check_from(uc: Self::Unchecked) -> (Self, Vec<Self::Err>) {
        let (registry, errs) = uc.registry.filter_check();

        let mut checked = Self { 
            registry,
            task_data: uc.task_data,
            phantom: PhantomData
        };

        let mut client_errs = Vec::new();
        for err in errs {
            let PairCheckError::Key(client_err, client_data) = err;

            checked.task_data.pending_work -= client_data.pending_work;
            client_errs.push(client_err);
        }

        (checked, client_errs)
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