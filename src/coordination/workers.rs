use std::{collections::{HashMap, hash_map}, fmt::Debug, hash::Hash};

use derive_where::derive_where;
use log::warn;
use screeps::{Creep, game};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json_any_key::any_key_map;

use crate::{check::{Check, CheckFrom, FilterCheck, FilterCheckFrom, PairCheckError}, ids::{Handle, WithId}};

const TIMEOUT: u32 = 2;

#[derive(Serialize, Deserialize)]
struct WorkerState<WorkerData> {
    last_heartbeat: u32,
    data: WorkerData
}

pub enum WorkerStateCheckError<WorkerData: CheckFrom> {
    Timeout(WorkerData),
    DataCheck(WorkerData::Err)
}

impl<WD: CheckFrom> CheckFrom for WorkerState<WD> {
    type Unchecked = WorkerState<WD::Unchecked>;
    type Err = WorkerStateCheckError<WD>;

    fn check_from(uc: Self::Unchecked) -> Result<Self, Self::Err> {
        let data = uc.data.check().map_err(WorkerStateCheckError::DataCheck)?;
        if game::time() > uc.last_heartbeat + TIMEOUT { return Err(WorkerStateCheckError::Timeout(data)) }

        Ok(Self {
            data,
            .. uc
        })
    }
}

impl<WD> WorkerState<WD> {
    pub fn new(data: WD) -> Self {
        Self { last_heartbeat: game::time(), data }
    }
}

#[derive_where(Serialize; Worker, WorkerData, Worker: Hash + Eq + 'static)]
#[derive_where(Deserialize; Worker: Hash + Eq + DeserializeOwned + 'static, WorkerData: DeserializeOwned + 'static)]
pub struct Workers<WorkerData, Worker = Handle<WithId<Creep>>> {
    #[serde(with = "any_key_map")] 
    workers: HashMap<Worker, WorkerState<WorkerData>>
}

impl<WD, W> IntoIterator for Workers<WD, W> {
    type Item = (W, WD);
    type IntoIter = impl Iterator<Item = Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.workers.into_iter().map(|(c, cr)| (c, cr.data))
    }
}

impl<WD, W> Workers<WD, W> {
    pub fn new() -> Self {
        Self { workers: HashMap::new() }
    }
}

impl<WorkerData, Worker> Workers<WorkerData, Worker> where Worker : Hash + Eq {    
    pub fn add(&mut self, worker: Worker, data: WorkerData) -> Option<WorkerData> {
        self.workers.insert(worker, WorkerState::new(data)).map(|state| state.data)
    }

    pub fn heartbeat(&mut self, worker: Worker) -> Option<WorkerHandle<'_, Worker, WorkerData>> {
        match self.workers.entry(worker) {
            hash_map::Entry::Vacant(_) => None,
            hash_map::Entry::Occupied(mut entry) => {
                entry.get_mut().last_heartbeat = game::time();
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

pub struct WorkerHandle<'a, Worker, WorkerData>(hash_map::OccupiedEntry<'a, Worker, WorkerState<WorkerData>>);

impl<W, WD> WorkerHandle<'_, W, WD> {
    pub fn get(&self) -> &WD {
        &self.0.get().data
    }

    pub fn get_mut(&mut self) -> &mut WD {
        &mut self.0.get_mut().data
    }

    pub fn remove(self) {
        self.0.remove();
    }
}

pub enum WorkerEntryCheckError<Worker: CheckFrom, WorkerData: CheckFrom> {
    Worker(Worker::Err, WorkerData::Unchecked),
    Data(Worker, WorkerData::Err),
    Timeout(Worker, WorkerData)
}

impl<WorkerData, Worker> FilterCheckFrom for Workers<WorkerData, Worker> 
where
    Worker: CheckFrom + Hash + Eq + Debug,
    WorkerData: CheckFrom
{
    type Unchecked = Workers<WorkerData::Unchecked, Worker::Unchecked>;
    type Err = WorkerEntryCheckError<Worker, WorkerData>;
    
    fn filter_check_from(uc: Self::Unchecked) -> (Self, Vec<Self::Err>) {
        let (workers, errs) = uc.workers.filter_check();
        for err in &errs {
            if let PairCheckError::Value(worker, WorkerStateCheckError::Timeout(_)) = &err {
                warn!("{worker:?} timed out");
            }
        }

        let errs = errs.into_iter().map(|err| {
            match err {
                PairCheckError::Key(worker_error, worker_entry) => {
                    let worker_entry: WorkerState<WorkerData::Unchecked> = worker_entry; 
                    WorkerEntryCheckError::Worker(worker_error, worker_entry.data)
                },
                PairCheckError::Value(worker, WorkerStateCheckError::DataCheck(data_err)) => 
                    WorkerEntryCheckError::Data(worker, data_err),
                PairCheckError::Value(worker, WorkerStateCheckError::Timeout(data)) =>
                    WorkerEntryCheckError::Timeout(worker, data)
            }
        }).collect();

        (Self { workers }, errs)
    }
}