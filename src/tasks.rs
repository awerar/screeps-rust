use std::{collections::{HashMap, HashSet, hash_map}, fmt::Debug, hash::Hash};

use itertools::Itertools;
use log::warn;
use screeps::{Creep, MaybeHasId, ObjectId, game};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json_any_key::any_key_map;

use crate::statemachine::UnderlyingName;

#[derive(Debug, Serialize, Deserialize)]
#[serde(bound = "D: Serialize + DeserializeOwned")]
struct TaskData<D> {
    target: TaskAmount,
    pending: TaskAmount,
    data: D,

    creeps: HashMap<ObjectId<Creep>, CreepData>
}

impl<D> TaskData<D> {
    fn new(target: TaskAmount, data: D) -> Self {
        Self { 
            target,
            data,
            pending: 0, 
            creeps: HashMap::new()
        }
    }

    fn left(&self) -> TaskAmount {
        self.target.saturating_sub(self.pending)
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct CreepData {
    contribution: TaskAmount,
    last_heartbeat: u32
}

pub type TaskAmount = u32;

#[derive(Serialize, Deserialize)]
#[serde(bound = "R: Serialize + DeserializeOwned + Eq + Hash, D: Serialize + DeserializeOwned")]
pub struct TaskServer<R, D, const TIMEOUT: u32 = 5>(#[serde(with = "any_key_map")] HashMap<R, TaskData<D>>) where R : 'static, D : 'static;

impl<R, D> Default for TaskServer<R, D> where R : Serialize + DeserializeOwned + Eq + Hash {
    fn default() -> Self {
        Self(HashMap::new())
    }
}

impl<T : Hash + Eq + Clone, D, const TIMEOUT: u32> TaskServer<T, D, TIMEOUT> {
    pub fn handle_timeouts(&mut self) {
        let timed_out_creeps = self.0.iter()
            .flat_map(|(task, task_data)| 
                task_data.creeps.iter()
                    .filter(|(_, creep_data)| creep_data.last_heartbeat + TIMEOUT <= game::time())
                    .map(|(creep, _)| (*creep, task.clone()))
            ).collect_vec();

        for (creep, task) in timed_out_creeps {
            if let Some(creep) = creep.resolve() {
                warn!("{} still exists, but timed out on task", creep.name());
            }

            self.finish_task(creep, &task, false);
        }
    }

    pub fn start_task(&mut self, creep: &Creep, task: &T, contribution: TaskAmount) -> bool {
        let Some(task_data) = self.0.get_mut(task) else { return false };
        task_data.creeps.insert(creep.try_id().unwrap(), CreepData { contribution, last_heartbeat: game::time() });
        task_data.pending += contribution;
        true
    }

    pub fn heartbeat_task(&mut self, creep: &Creep, task: &T) -> bool {
        let Some(task_data) = self.0.get_mut(task) else { return false };
        let Some(creep) = task_data.creeps.get_mut(&creep.try_id().unwrap()) else { return false };
        creep.last_heartbeat = game::time();
        true
    }

    pub fn finish_task(&mut self, creep: ObjectId<Creep>, task: &T, success: bool) {
        let Some(task_data) = self.0.get_mut(task) else { return };
        let Some(creep_data) = task_data.creeps.remove(&creep) else { return };

        task_data.pending = task_data.pending.checked_sub(creep_data.contribution).unwrap();

        if !success { return }
        task_data.target = task_data.target.saturating_sub(creep_data.contribution);

        if task_data.target > 0 { return; }
        self.0.remove(task);
    }

    pub fn assign_task<F>(&mut self, creep: &Creep, contribution: TaskAmount, picker: F) -> Option<T>
        where for<'a> F : FnOnce(Vec<(&'a T, TaskAmount, &D)>) -> Option<&'a T>
    {
        let task = { picker(self.get_avaliable_tasks().collect())?.clone() };
        self.start_task(creep, &task, contribution);
        Some(task)
    }

    pub fn set_tasks(&mut self, new_tasks: impl IntoIterator<Item = (T, TaskAmount, D)>) {
        let new_tasks = new_tasks.into_iter().filter(|(_, target, _)| *target > 0).collect_vec();

        let new_task_set: HashSet<_> = new_tasks.iter()
            .map(|(task, _, _)| task.clone())
            .collect();
        let old_task_set: HashSet<_> = self.0.keys().cloned().collect();
        let removed_tasks = old_task_set.difference(&new_task_set);
        
        for task in removed_tasks {
            self.0.remove(task);
        }

        for (task, target, data) in new_tasks {
            match self.0.entry(task) {
                hash_map::Entry::Occupied(mut entry) => {
                    entry.get_mut().target = target;
                },
                hash_map::Entry::Vacant(entry) => { 
                    entry.insert(TaskData::new(target, data)); 
                },
            }
        }
    }

    pub fn get_avaliable_tasks(&self) -> impl Iterator<Item = (&T, TaskAmount, &D)> {
        self.0.iter()
            .map(|(task, task_data)| (task, task_data.left(), &task_data.data))
            .filter(|(_, left, _)| *left > 0)
    }
}