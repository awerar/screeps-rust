use std::{any::Any, collections::{HashMap, HashSet, hash_map}, fmt::Debug, hash::Hash};

use derive_where::derive_where;
use itertools::{Either, Itertools};
use log::warn;
use screeps::{Creep, game};
use serde::{Deserialize, Deserializer, Serialize, de::DeserializeOwned};
use serde_json_any_key::any_key_map;

use crate::{check::{Check, CheckFrom}, ids::{CheckState, Checked, Handle, Unchecked, WithId}};

#[derive(Debug)]
#[derive_where(Serialize, Deserialize; Handle<WithId<Creep>, I>, D)]
struct TaskData<D, I : CheckState = Checked> {
    target: TaskAmount,
    pending: TaskAmount,
    data: D,

    creeps: HashMap<Handle<WithId<Creep>, I>, CreepData>
}

impl<'de, D : DeserializeOwned> Deserialize<'de> for TaskData<D> {
    fn deserialize<De: Deserializer<'de>>(deserializer: De) -> Result<Self, De::Error> {
        let raw = TaskData::<D, Unchecked>::deserialize(deserializer)?;

        let (safe_creeps, unsafe_contributions): (HashMap<_, _>, Vec<_>) = raw.creeps.into_iter()
            .partition_map(|(creep, creep_data)| {
                if let Ok(creep) = creep.check() {
                    Either::Left((creep, creep_data))
                } else {
                    Either::Right(creep_data.contribution)
                }
            });

        Ok(TaskData {
            target: raw.target,
            pending: raw.pending - unsafe_contributions.into_iter().sum::<u32>(),
            creeps: safe_creeps,
            data: raw.data
        })
    }
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
#[serde(bound(serialize = "R: Serialize + Any, TaskData<D> : Serialize"))]
#[serde(bound(deserialize = "R: DeserializeOwned + Eq + Hash + Any, TaskData<D> : DeserializeOwned, D : Any"))]
pub struct TaskServer<R, D, const TIMEOUT: u32 = 5>(
    #[serde(with = "any_key_map")] 
    HashMap<R, TaskData<D>>
);

#[expect(private_bounds)]
pub fn prune_deserialize_taskserver<'de, R, D, De>(deserializer: De) -> Result<TaskServer<R, D>, De::Error>
where
    De: Deserializer<'de>,
    R: CheckFrom + Eq + Hash,
    R::Unchecked: DeserializeOwned + Eq + Hash + Any,
    TaskData<D> : DeserializeOwned,
    D : Any
{
    let raw = TaskServer::<R::Unchecked, D>::deserialize(deserializer)?;
    Ok(TaskServer(raw.0.into_iter().filter_map(|(k, v)| Some((k.check().ok()?, v))).collect()))
}

impl<R, D> Default for TaskServer<R, D> {
    fn default() -> Self {
        Self(HashMap::default())
    }
}

// TODO: Gradual progress improvments
impl<T : Hash + Eq + Clone, D, const TIMEOUT: u32> TaskServer<T, D, TIMEOUT> {
    pub fn handle_timeouts(&mut self) {
        let timed_out_creeps = self.0.iter()
            .flat_map(|(task, task_data)| 
                task_data.creeps.iter()
                    .filter(|(_, creep_data)| creep_data.last_heartbeat + TIMEOUT <= game::time())
                    .map(|(creep, _)| (creep.clone(), task.clone()))
            ).collect_vec();

        for (creep, task) in timed_out_creeps {
            warn!("{} timed out on task", creep.name());

            self.finish_task(&creep, &task, false);
        }
    }

    pub fn start_task(&mut self, creep: Handle<WithId<Creep>>, task: &T, contribution: TaskAmount) -> bool {
        let Some(task_data) = self.0.get_mut(task) else { return false };
        task_data.creeps.insert(creep, CreepData { contribution, last_heartbeat: game::time() });
        task_data.pending += contribution;
        true
    }

    pub fn heartbeat_task(&mut self, creep: &Handle<WithId<Creep>>, task: &T) -> bool {
        let Some(task_data) = self.0.get_mut(task) else { return false };
        let Some(creep) = task_data.creeps.get_mut(creep) else { return false };
        creep.last_heartbeat = game::time();
        true
    }

    pub fn finish_task(&mut self, creep: &Handle<WithId<Creep>>, task: &T, success: bool) {
        let Some(task_data) = self.0.get_mut(task) else { return };
        let Some(creep_data) = task_data.creeps.remove(creep) else { return };

        task_data.pending = task_data.pending.checked_sub(creep_data.contribution).unwrap();

        if !success { return }
        task_data.target = task_data.target.saturating_sub(creep_data.contribution);

        if task_data.target > 0 { return; }
        self.0.remove(task);
    }

    pub fn assign_task<F>(&mut self, creep: Handle<WithId<Creep>>, contribution: TaskAmount, picker: F) -> Option<T>
        where for<'a> F : FnOnce(Vec<(&'a T, TaskAmount, &'a D)>) -> Option<(&'a T, TaskAmount, &'a D)>
    {
        let task = { picker(self.get_avaliable_tasks().collect())?.0.clone() };
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
                    entry.get_mut().data = data;
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