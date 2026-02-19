use std::{collections::{HashMap, HashSet, hash_map}, fmt::Debug, hash::Hash};

use itertools::Itertools;
use log::warn;
use screeps::{Creep, HasPosition, MaybeHasId, ObjectId, Position, game};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::statemachine::UnderlyingName;

#[derive(Debug, Serialize, Deserialize)]
struct TaskData {
    target: TaskAmount,
    pending: TaskAmount,
    priority: TaskPriority,
    pos: Position,

    creeps: HashMap<ObjectId<Creep>, CreepData>
}

impl TaskData {
    fn new(pos: Position, target: TaskAmount, priority: TaskPriority) -> Self {
        Self { 
            target,
            pos,
            pending: 0, 
            priority,
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

pub type TaskPriority = u32;
pub type TaskAmount = u32;

pub struct ScheduledTask<T> {
    pub priority: TaskPriority,
    pub target: TaskAmount,
    pub pos: Position,
    pub task: T
}

#[derive(Serialize, Deserialize)]
#[serde(bound = "R: Serialize + DeserializeOwned + Eq + Hash")]
pub struct MultiTasksQueue<R, const TIMEOUT: u32 = 5>(HashMap<R, TaskData>);

impl<R> Default for MultiTasksQueue<R> where R : Serialize + DeserializeOwned + Eq + Hash {
    fn default() -> Self {
        Self(HashMap::new())
    }
}

impl<T : Hash + Eq + Clone + Debug, const TIMEOUT: u32> MultiTasksQueue<T, TIMEOUT> {
    pub fn handle_timeouts(&mut self) {
        let timed_out_creeps = self.0.iter()
            .flat_map(|(task, task_data)| 
                task_data.creeps.iter()
                    .filter(|(_, creep_data)| creep_data.last_heartbeat + TIMEOUT <= game::time())
                    .map(|(creep, _)| (creep.clone(), task.clone()))
            ).collect_vec();

        for (creep, task) in timed_out_creeps {
            if let Some(creep) = creep.resolve() {
                warn!("{} still exists, but timed out on task", creep.name());
            }

            self.finish(creep, &task, false);
        }
    }

    pub fn heartbeat(&mut self, creep: &Creep, task: &T) -> bool {
        let Some(task) = self.0.get_mut(task) else { return false };
        task.creeps.get_mut(&creep.try_id().unwrap()).unwrap().last_heartbeat = game::time();
        true
    }

    pub fn finish(&mut self, creep: ObjectId<Creep>, task: &T, success: bool) {
        let Some(task_data) = self.0.get_mut(task) else { return };
        let Some(creep_data) = task_data.creeps.remove(&creep) else { return };

        task_data.pending = task_data.pending.checked_sub(creep_data.contribution).unwrap();

        if !success { return }
        task_data.target = task_data.target.saturating_sub(creep_data.contribution);

        if task_data.target > 0 { return; }
        self.0.remove(task);
    }

    pub fn assign_task_to(&mut self, creep: &Creep, contribution: TaskAmount, allow_under_contribution: bool) -> Option<T> {
        let task = self.0.iter_mut()
            .filter(|(_, task_data)| task_data.left() > 0 && (allow_under_contribution || task_data.left() >= contribution))
            .max_set_by_key(|(_, task_data)| task_data.priority)
            .into_iter()
            .min_by_key(|(_, task_data)| task_data.pos.get_range_to(creep.pos()));

        let Some((task, task_data)) = task else { return None };
        task_data.pending += contribution;
        task_data.creeps.insert(creep.try_id().unwrap(), CreepData { contribution, last_heartbeat: game::time() });

        Some(task.clone())
    }

    pub fn set_tasks(&mut self, new_tasks: impl IntoIterator<Item = ScheduledTask<T>>) {
        let new_tasks = new_tasks.into_iter().filter(|scheduled_task| scheduled_task.target > 0).collect_vec();

        let new_task_set: HashSet<_> = new_tasks.iter()
            .map(|scheduled_task| scheduled_task.task.clone())
            .collect();
        let old_task_set: HashSet<_> = self.0.keys().cloned().collect();
        let removed_tasks = old_task_set.difference(&new_task_set);
        
        for task in removed_tasks {
            self.0.remove(task);
        }

        for scheduled_task in new_tasks {
            match self.0.entry(scheduled_task.task) {
                hash_map::Entry::Occupied(mut entry) => {
                    entry.get_mut().target = scheduled_task.target;
                    entry.get_mut().priority = scheduled_task.priority;
                },
                hash_map::Entry::Vacant(entry) => { 
                    entry.insert(TaskData::new(scheduled_task.pos, scheduled_task.target, scheduled_task.priority)); 
                },
            }
        }
    }
}