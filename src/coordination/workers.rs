use std::{collections::{HashMap, hash_map}, hash::Hash};

use derive_where::derive_where;
use log::warn;
use screeps::Creep;
use serde::de::DeserializeOwned;
use serde_json_any_key::any_key_map;

use crate::{check::{CheckFrom, Expiring, ExpiringCheckError, FilterCheck, FilterCheckFrom, PairCheckError}, domain_traits::HasName, ids::{CheckState, Checked, Handle, Unchecked, WithId}};

const TIMEOUT: u32 = 1;

#[derive_where(Serialize; Worker, WorkerData, Worker: Hash + Eq + 'static, S)]
#[derive_where(Deserialize; Worker: Hash + Eq + DeserializeOwned + 'static, WorkerData: DeserializeOwned + 'static, S: DeserializeOwned)]
pub struct Workers<WorkerData = (), Worker = Handle<WithId<Creep>>, S: CheckState = Checked> {
    #[serde(with = "any_key_map")] 
    workers: HashMap<Worker, Expiring<WorkerData, TIMEOUT, S>>
}

impl<WD, W> IntoIterator for Workers<WD, W> {
    type Item = (W, WD);
    type IntoIter = impl Iterator<Item = Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.workers.into_iter().map(|(c, cr)| (c, cr.inner))
    }
}

impl<WD, W> Workers<WD, W> {
    pub fn new() -> Self {
        Self { workers: HashMap::new() }
    }
}

impl<WorkerData, Worker> Workers<WorkerData, Worker> where Worker : Hash + Eq {    
    pub fn add(&mut self, worker: Worker, data: WorkerData) -> Option<WorkerData> {
        self.workers.insert(worker, Expiring::new(data)).map(|expiry| expiry.inner)
    }

    pub fn heartbeat(&mut self, worker: Worker) -> Option<WorkerHandle<'_, WorkerData, Worker>> {
        match self.workers.entry(worker) {
            hash_map::Entry::Vacant(_) => None,
            hash_map::Entry::Occupied(mut entry) => {
                entry.get_mut().expiration.refresh();
                Some(WorkerHandle(entry))
            },
        }
    }
}

impl<WD, W> Default for Workers<WD, W> {
    fn default() -> Self {
        Self::new()
    }
}

pub struct WorkerHandle<'a, WorkerData = (), Worker = Handle<WithId<Creep>>>(hash_map::OccupiedEntry<'a, Worker, Expiring<WorkerData, TIMEOUT>>);

impl<WD, W> WorkerHandle<'_, WD, W> {
    pub fn get(&self) -> &WD {
        self.0.get()
    }

    pub fn get_mut(&mut self) -> &mut WD {
        self.0.get_mut()
    }

    pub fn remove(self) {
        self.0.remove();
    }
}

pub enum WorkerEntryCheckError<WorkerData: CheckFrom, Worker: CheckFrom> {
    Worker(Worker::Err, WorkerData::Unchecked),
    Data(Worker, WorkerData::Err),
    Timeout(Worker, WorkerData)
}

impl<WorkerData, Worker> FilterCheckFrom for Workers<WorkerData, Worker> 
where
    Worker: CheckFrom + Hash + Eq + HasName,
    WorkerData: CheckFrom
{
    type Unchecked = Workers<WorkerData::Unchecked, Worker::Unchecked, Unchecked>;
    type Err = WorkerEntryCheckError<WorkerData, Worker>;
    
    fn filter_check_from(uc: Self::Unchecked) -> (Self, Vec<Self::Err>) {
        let (workers, errs): (HashMap<Worker, _>, _) = uc.workers.filter_check();
        for err in &errs {
            if let PairCheckError::Value(worker, ExpiringCheckError::Expired(_)) = &err {
                warn!("{} timed out", worker.name());
            }
        }

        let errs = errs.into_iter().map(|err| {
            match err {
                PairCheckError::Key(worker_error, worker_expiry) => {
                    let worker_expiry: Expiring<WorkerData::Unchecked, TIMEOUT, Unchecked> = worker_expiry; 
                    WorkerEntryCheckError::Worker(worker_error, worker_expiry.inner)
                },
                PairCheckError::Value(worker, ExpiringCheckError::Inner(data_err)) => 
                    WorkerEntryCheckError::Data(worker, data_err),
                PairCheckError::Value(worker, ExpiringCheckError::Expired(data)) =>
                    WorkerEntryCheckError::Timeout(worker, data)
            }
        }).collect();

        (Self { workers }, errs)
    }
}