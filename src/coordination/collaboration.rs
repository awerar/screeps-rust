use std::{fmt::Debug, hash::Hash};

use derive_where::derive_where;
use serde::{Deserialize, Serialize};

use crate::{check::{CheckFrom, FilterCheck, FilterCheckFrom, TriviallyChecked}, coordination::{workers::{WorkerStateCheckError, WorkerEntryCheckError, WorkerHandle, Workers}, tasks::UpdateableTaskData}};

#[derive(Serialize, Deserialize)]
struct PendingWork(u32);

impl TriviallyChecked for PendingWork {}

#[derive(Serialize, Deserialize)]
pub struct TaskState {
    remaining_work: u32,
    pending_work: u32
}

#[derive_where(Serialize, Deserialize; Workers<Worker, PendingWork>)]
pub struct Collaboration<Worker> {
    registry: Workers<Worker, PendingWork>,
    task_data: TaskState
}

impl<C> Collaboration<C> {
    pub fn new(required_work: u32) -> Self {
        Self { 
            registry: Workers::new(),
            task_data: TaskState { 
                remaining_work: required_work, 
                pending_work: 0
            }
        }
    }
}

impl<Worker: Hash + Eq> Collaboration<Worker> {
    pub fn heartbeat(&mut self, worker: Worker) -> Option<CollaborativeWorkerHandle<'_, Worker>> {
        Some(CollaborativeWorkerHandle {
            worker_handle: self.registry.heartbeat(worker)?,
            task_data: &mut self.task_data
        })
    }

    // TODO: collisions
    pub fn add(&mut self, worker: Worker, work: u32) {
        if let Some(PendingWork(other_work)) = self.registry.add(worker, PendingWork(work)) {
            self.task_data.pending_work -= other_work;
        }

        self.task_data.pending_work += work;
    }
}

pub struct RemainingWork(pub u32);
impl<Worker> UpdateableTaskData for Collaboration<Worker> {
    type Update = RemainingWork;

    fn update(&mut self, update: Self::Update) {
        self.task_data.remaining_work = update.0;
    }

    fn create(update: Self::Update) -> Self {
        Self::new(update.0)
    }
}

pub enum WorkerCheckError<Worker: CheckFrom> {
    Timeout(Worker),
    WorkerCheck(Worker::Err)
}

impl<Worker: CheckFrom + Hash + Eq + Debug> FilterCheckFrom for Collaboration<Worker> {
    type Unchecked = Collaboration<Worker::Unchecked>;
    type Err = WorkerCheckError<Worker>;

    fn filter_check_from(uc: Self::Unchecked) -> (Self, Vec<Self::Err>) {
        let (registry, errs) = uc.registry.filter_check();

        let mut checked = Self { 
            registry,
            task_data: uc.task_data
        };

        let mut new_errs = Vec::new();
        for err in errs {
            let (pending_work, new_err) = match err {
                WorkerEntryCheckError::Worker(worker_err, worker_data) => {
                    (worker_data, WorkerCheckError::WorkerCheck(worker_err))
                },
                WorkerEntryCheckError::Timeout(worker, worker_data) => 
                    (worker_data, WorkerCheckError::Timeout(worker)),
            };

            checked.task_data.pending_work -= pending_work.0;
            new_errs.push(new_err);
        }

        (checked, new_errs)
    }
}

pub struct CollaborativeWorkerHandle<'a, Worker> {
    task_data: &'a mut TaskState,
    worker_handle: WorkerHandle<'a, Worker, PendingWork>
}

impl<Worker> CollaborativeWorkerHandle<'_, Worker> {
    pub fn apply_work(&mut self, amount: u32) {
        self.task_data.pending_work = self.task_data.pending_work.saturating_sub(amount);
        self.task_data.remaining_work = self.task_data.remaining_work.saturating_sub(amount);
        self.worker_handle.get_mut().0 = self.worker_handle.get().0.saturating_sub(amount);
    }

    pub fn remove(self) {
        self.task_data.pending_work += self.worker_handle.get().0;
        self.worker_handle.remove();
    }

    pub fn remaining(&self) -> u32 {
        self.worker_handle.get().0
    }
}