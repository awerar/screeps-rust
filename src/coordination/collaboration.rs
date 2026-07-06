use std::hash::Hash;

use derive_where::derive_where;
use screeps::Creep;
use serde::{Deserialize, Serialize};

use crate::{check::{Check, CheckFrom, FilterCheck, FilterCheckFrom}, coordination::{tasks::UpdateableTaskData, expiring_map::{ExpiringEntryCheckError, LiveHandle, ExpiringMap}}, domain_traits::HasName, ids::{CheckState, Checked, Handle, Unchecked, WithId}};

#[derive(Serialize, Deserialize)]
struct WorkerState<WorkerData> {
    pending_work: u32,
    data: WorkerData
}

struct WorkerStateCheckErr<WorkerData: CheckFrom> {
    pending_work: u32,
    err: WorkerData::Err
}

impl<WD: CheckFrom> CheckFrom for WorkerState<WD> {
    type Unchecked = WorkerState<WD::Unchecked>;
    type Err = WorkerStateCheckErr<WD>;

    fn check_from(uc: Self::Unchecked) -> Result<Self, Self::Err> {
        Ok(Self { 
            pending_work: uc.pending_work, 
            data: uc.data.check()
                .map_err(|err| 
                    WorkerStateCheckErr { 
                        pending_work: uc.pending_work, 
                        err
                    }
                )? 
        })
    }
}

#[derive(Serialize, Deserialize)]
struct TaskState {
    remaining_work: u32,
    pending_work: u32
}

#[derive_where(Serialize, Deserialize; ExpiringMap<Worker, WorkerState<WorkerData>, 1, S>)]
pub struct Collaboration<Worker, WorkerData = (), S: CheckState = Checked> {
    registry: ExpiringMap<Worker, WorkerState<WorkerData>, 1, S>,
    task_data: TaskState
}

pub type CreepCollaboration<WorkerData = (), S = Checked> = Collaboration<Handle<WithId<Creep>>, WorkerData, S>;

impl<W, WD> Collaboration<W, WD> {
    pub fn new(required_work: u32) -> Self {
        Self { 
            registry: ExpiringMap::new(),
            task_data: TaskState { 
                remaining_work: required_work, 
                pending_work: 0
            }
        }
    }

    pub fn unassigned_work(&self) -> u32 {
        self.task_data.remaining_work.saturating_sub(self.task_data.pending_work)
    }

    pub fn set_remaining_work(&mut self, remaining_work: u32) {
        self.task_data.remaining_work = remaining_work;
    }
}

impl<Worker: Hash + Eq, WorkerData> Collaboration<Worker, WorkerData> {
    pub fn heartbeat(&mut self, worker: Worker) -> Option<CollaborativeWorkerHandle<'_, Worker, WorkerData>> {
        Some(CollaborativeWorkerHandle {
            worker_handle: self.registry.refresh(worker)?,
            task_data: &mut self.task_data
        })
    }

    // TODO: collisions
    pub fn add(&mut self, worker: Worker, work: u32, data: WorkerData) {
        if let Some(other_state) = self.registry.add(worker, WorkerState { pending_work: work, data }) {
            self.task_data.pending_work -= other_state.pending_work;
        }

        self.task_data.pending_work += work;
    }
}

pub struct RemainingWork(pub u32);
impl<Worker, WorkerData> UpdateableTaskData for Collaboration<Worker, WorkerData> {
    type Update = RemainingWork;

    fn update(&mut self, update: Self::Update) {
        self.set_remaining_work(update.0);
    }

    fn create(update: Self::Update) -> Self {
        Self::new(update.0)
    }
}

impl<Worker, WorkerData> FilterCheckFrom for Collaboration<Worker, WorkerData> 
where 
    WorkerData: CheckFrom,
    Worker: CheckFrom + Hash + Eq + HasName
{
    type Unchecked = Collaboration<Worker::Unchecked, WorkerData::Unchecked, Unchecked>;
    type Err = ExpiringEntryCheckError<Worker, WorkerData>;

    fn filter_check_from(uc: Self::Unchecked) -> (Self, Vec<Self::Err>) {
        let (registry, errs) = uc.registry.filter_check();

        let mut checked = Self { 
            registry,
            task_data: uc.task_data
        };

        let mut new_errs = Vec::new();
        for err in errs {
            let (pending_work, new_err) = match err {
                ExpiringEntryCheckError::Key(worker_err, worker_state) => {
                    (worker_state.pending_work, ExpiringEntryCheckError::Key(worker_err, worker_state.data))
                },
                ExpiringEntryCheckError::Value(worker, worker_state_err) => {
                    (worker_state_err.pending_work, ExpiringEntryCheckError::Value(worker, worker_state_err.err))
                },
                ExpiringEntryCheckError::Expired(worker, worker_state) => 
                    (worker_state.pending_work, ExpiringEntryCheckError::Expired(worker, worker_state.data)),
            };

            checked.task_data.pending_work -= pending_work;
            new_errs.push(new_err);
        }

        (checked, new_errs)
    }
}

pub struct CollaborativeWorkerHandle<'a, Worker = Handle<WithId<Creep>>, WorkerData = ()> {
    task_data: &'a mut TaskState,
    worker_handle: LiveHandle<'a, Worker, WorkerState<WorkerData>>
}

pub type CollaborativeCreepHandle<'a, WorkerData = ()> = CollaborativeWorkerHandle<'a, Handle<WithId<Creep>>, WorkerData>;

impl<Worker, WorkerData> CollaborativeWorkerHandle<'_, Worker, WorkerData> {
    pub fn apply_work(&mut self, amount: u32) {
        self.task_data.pending_work = self.task_data.pending_work.saturating_sub(amount);
        self.task_data.remaining_work = self.task_data.remaining_work.saturating_sub(amount);
        self.worker_handle.get_mut().pending_work = self.worker_handle.get().pending_work.saturating_sub(amount);
    }

    pub fn remove(self) {
        self.task_data.pending_work += self.worker_handle.get().pending_work;
        self.worker_handle.remove();
    }

    pub fn remaining(&self) -> u32 {
        self.worker_handle.get().pending_work
    }

    #[expect(unused)]
    pub fn get(&self) -> &WorkerData {
        &self.worker_handle.get().data
    }

    #[expect(unused)]
    pub fn get_mut(&mut self) -> &mut WorkerData {
        &mut self.worker_handle.get_mut().data
    }
}