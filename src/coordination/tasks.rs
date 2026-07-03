use std::{collections::{HashMap, HashSet, hash_map}, hash::Hash};

use derive_where::derive_where;
use serde::de::DeserializeOwned;
use serde_json_any_key::any_key_map;

use crate::check::{CheckFrom, FilterCheck, FilterCheckFrom, PairCheckError};

#[derive_where(Serialize; Task, TaskData, Task: Hash + Eq + 'static)]
#[derive_where(Deserialize; Task: Hash + Eq + DeserializeOwned + 'static, TaskData: DeserializeOwned + 'static)]
pub struct Tasks<Task, TaskData> {
    #[serde(with = "any_key_map")] 
    tasks: HashMap<Task, TaskData>
}

impl<Task: Hash + Eq + Clone, TaskData: UpdateableTaskData> Tasks<Task, TaskData> {
    pub fn set(&mut self, new_tasks: HashMap<Task, TaskData::Update>) {
        self.tasks.keys().cloned().collect::<HashSet<_>>()
            .difference(&new_tasks.keys().cloned().collect())
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

    pub fn get(&self, task: &Task) -> Option<&TaskData> {
        self.tasks.get(task)
    }

    pub fn get_mut(&mut self, task: &Task) -> Option<&mut TaskData> {
        self.tasks.get_mut(task)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&Task, &TaskData)> {
        self.tasks.iter()
    }
}

pub trait UpdateableTaskData {
    type Update;

    fn update(&mut self, update: Self::Update);
    fn create(update: Self::Update) -> Self;
}

impl<Task: CheckFrom + Hash + Eq, TaskData: CheckFrom> FilterCheckFrom for Tasks<Task, TaskData> {
    type Unchecked = Tasks<Task::Unchecked, TaskData::Unchecked>;
    type Err = PairCheckError<Task, TaskData>;

    fn filter_check_from(uc: Self::Unchecked) -> (Self, Vec<Self::Err>) {
        let (tasks, errs) = uc.tasks.filter_check();
        (Self { tasks }, errs)
    }
}