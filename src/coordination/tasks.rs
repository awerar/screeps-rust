use std::{collections::{HashMap, HashSet, hash_map}, hash::Hash};

use derive_where::derive_where;
use screeps::Position;
use serde::de::DeserializeOwned;
use serde_json_any_key::any_key_map;

use crate::{check::{CheckFrom, FilterCheck, FilterCheckFrom, Filtered, PairCheckError}, coordination::collaboration::{Collaboration, CollaborativeWorkerHandle}};

#[derive_where(Serialize; Task, TaskData, Task: Hash + Eq + 'static)]
#[derive_where(Deserialize; Task: Hash + Eq + DeserializeOwned + 'static, TaskData: DeserializeOwned + 'static)]
#[derive_where(Default)]
pub struct Tasks<Task, TaskData> {
    #[serde(with = "any_key_map")] 
    tasks: HashMap<Task, TaskData>
}

impl<Task: Hash + Eq + Clone, TaskData: UpdateableTaskData> Tasks<Task, TaskData> {
    pub fn set_tasks(&mut self, new_tasks: impl IntoIterator<Item = (Task, TaskData::Update)>) {
        let new_tasks: Vec<_> = new_tasks.into_iter().collect();

        self.tasks.keys().cloned().collect::<HashSet<_>>()
            .difference(&new_tasks.iter().map(|x| x.0.clone()).collect())
            .for_each(|removed_task| {
                self.tasks.remove(removed_task);
            });

        for (task, update) in new_tasks {
            match self.tasks.entry(task) {
                hash_map::Entry::Occupied(mut entry) => 
                    entry.get_mut().update(update),
                hash_map::Entry::Vacant(entry) => {
                    entry.insert(TaskData::create(update));
                },
            }
        }        
    }
}

impl<Task: Hash + Eq, TaskData> Tasks<Task, TaskData> {
    #[expect(unused)]
    pub fn get(&self, task: &Task) -> Option<&TaskData> {
        self.tasks.get(task)
    }

    pub fn get_mut(&mut self, task: &Task) -> Option<&mut TaskData> {
        self.tasks.get_mut(task)
    }

    #[expect(unused)]
    pub fn iter(&self) -> impl Iterator<Item = (&Task, &TaskData)> {
        self.tasks.iter()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&Task, &mut TaskData)> {
        self.tasks.iter_mut()
    }
}

impl<Task: Hash + Eq, K, Worker: Hash + Eq> Tasks<Task, (K, Filtered<Collaboration<Worker>>)> {
    pub fn heartbeat(&mut self, task: &Task, worker: Worker) -> Option<CollaborativeWorkerHandle<'_, Worker>> {
        self.get_mut(task).and_then(|(_, collab)| collab.heartbeat(worker))
    }
}

pub trait UpdateableTaskData {
    type Update;

    fn create(update: Self::Update) -> Self;
    fn update(&mut self, update: Self::Update);
}

impl<T: UpdateableTaskData> UpdateableTaskData for Filtered<T> {
    type Update = T::Update;

    fn create(update: Self::Update) -> Self { Filtered(T::create(update)) }
    fn update(&mut self, update: Self::Update) { self.0.update(update); }
}

impl<A: UpdateableTaskData, B: UpdateableTaskData> UpdateableTaskData for (A, B) {
    type Update = (A::Update, B::Update);

    fn create(update: Self::Update) -> Self {
        (A::create(update.0), B::create(update.1))
    }

    fn update(&mut self, update: Self::Update) {
        self.0.update(update.0);
        self.1.update(update.1);
    }
}

pub trait OverwriteableTaskData {}
impl OverwriteableTaskData for Position {}

impl<T: OverwriteableTaskData> UpdateableTaskData for T {
    type Update = Self;

    fn create(update: Self::Update) -> Self {
        update
    }

    fn update(&mut self, update: Self::Update) {
        *self = update;
    }
}

impl<Task: CheckFrom + Hash + Eq, TaskData: CheckFrom> FilterCheckFrom for Tasks<Task, TaskData> {
    type Unchecked = Tasks<Task::Unchecked, TaskData::Unchecked>;
    type Err = PairCheckError<Task, TaskData>;

    fn filter_check_from(uc: Self::Unchecked) -> (Self, Vec<Self::Err>) {
        let (tasks, errs) = uc.tasks.filter_check();
        (Self { tasks }, errs)
    }
}

pub trait AddedToCollab { 
    type Result;
    type Worker;

    fn added_to_collab(self, client: Self::Worker, amount: u32) -> Self::Result;
}

impl<T: Clone, K, Worker: Hash + Eq> AddedToCollab for Option<(&T, &mut (K, Filtered<Collaboration<Worker>>))> {
    type Result = Option<T>;
    type Worker = Worker;

    fn added_to_collab(self, client: Self::Worker, amount: u32) -> Self::Result {
        self.map(|(task, (_, collab))| {
            collab.add(client, amount);
            task.clone()
        })
    }
}