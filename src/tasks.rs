use std::{assert_matches, collections::{HashMap, HashSet, VecDeque, hash_map}, fmt::Debug, hash::Hash};

use itertools::Itertools;
use log::warn;
use screeps::{Creep, MaybeHasId, ObjectId, game};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::statemachine::UnderlyingName;

#[derive(Debug, Serialize, Deserialize)]
pub struct TaskData {
    target: u32,
    pending: u32
}

impl TaskData {
    fn new(target: u32) -> Self {
        Self { target, pending: 0 }
    }

    fn left(&self) -> u32 {
        self.target.saturating_sub(self.pending)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreepData<R> {
    current_task: R,
    contribution: u32,
    last_heartbeat: u32
}

impl<T> CreepData<T> {
    fn new(task: T, contribution: u32) -> Self {
        Self { current_task: task, contribution, last_heartbeat: game::time() }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(bound = "R: Serialize + DeserializeOwned + Eq + Hash")]
pub struct MultiTasksQueue<R, const TIMEOUT: u32 = 5> {
    task_queue: VecDeque<R>,

    tasks: HashMap<R, TaskData>,
    creeps: HashMap<ObjectId<Creep>, CreepData<R>>
}

impl<R> Default for MultiTasksQueue<R> where R : Serialize + DeserializeOwned + Eq + Hash {
    fn default() -> Self {
        Self { task_queue: VecDeque::new(), tasks: HashMap::new(), creeps: HashMap::new() }
    }
}

impl<T, const TIMEOUT: u32> MultiTasksQueue<T, TIMEOUT> where T : Hash + Eq + Clone + Debug {
    pub fn handle_timeouts(&mut self) {
        let timed_out_creeps = self.creeps.iter()
            .filter(|(_, data)| data.last_heartbeat + TIMEOUT <= game::time())
            .map(|(creep, _)| creep)
            .copied()
            .collect_vec();

        for creep in timed_out_creeps {
            if let Some(creep) = creep.resolve() {
                warn!("{} still exists, but timed out on task", creep.name());
            }

            self.finish(creep, false);
        }
    }

    pub fn heartbeat(&mut self, creep: &Creep) -> bool {
        let Some(creep) = self.creeps.get_mut(&creep.try_id().unwrap()) else { return false };
        creep.last_heartbeat = game::time();
        true
    }

    pub fn finish(&mut self, creep: ObjectId<Creep>, success: bool) {
        let Some(creep_data) = self.creeps.remove(&creep) else { return };
        let task_data = self.tasks.get_mut(&creep_data.current_task).unwrap();

        task_data.pending = task_data.pending.checked_sub(creep_data.contribution).unwrap();

        if !success { return }
        task_data.target = task_data.target.saturating_sub(creep_data.contribution);

        if task_data.target > 0 { return; }
        let task_index = self.task_queue.iter().find_position(|task| **task == creep_data.current_task).unwrap().0;
        self.task_queue.remove(task_index);
    }

    pub fn assign_task_to(&mut self, creep: ObjectId<Creep>, contribution: u32, allow_under_contribution: bool) -> Option<T> {
        if self.creeps.contains_key(&creep) { self.finish(creep, false); }

        let (i, task) = if allow_under_contribution  {
            self.task_queue.front().map(|task| (0, task.clone()))
        } else { 
            self.task_queue.iter().enumerate()
                .find(|(_, task)| self.tasks.get(*task).unwrap().left() >= contribution)
                .map(|(i, task)| (i, task.clone()))
        }?;

        let task_data = self.tasks.get_mut(&task).unwrap();
        task_data.pending += contribution;

        assert_matches!(self.creeps.insert(creep, CreepData::new(task.clone(), contribution)), None);

        if task_data.left() == 0 {
            self.task_queue.remove(i);
        }
        
        Some(task)
    }

    pub fn set_tasks(&mut self, new_tasks: impl IntoIterator<Item = (T, u32)>) {
        let new_tasks = new_tasks.into_iter().filter(|(_, target)| *target > 0).collect_vec();
        self.task_queue = new_tasks.iter().map(|(task, _)| task.clone()).collect::<VecDeque<_>>();

        let new_task_set: HashSet<_> = new_tasks.iter()
            .map(|(task, _)| task.clone())
            .collect();
        let old_task_set: HashSet<_> = self.tasks.keys().cloned().collect();
        let removed_tasks = old_task_set.difference(&new_task_set);
        
        for task in removed_tasks {
            self.tasks.remove(task);
            let removed_creeps = self.creeps.iter()
                .filter(|(_, creep_data)| creep_data.current_task == *task)
                .map(|(creep, _)| *creep)
                .collect_vec();

            for creep in removed_creeps {
                self.creeps.remove(&creep);
            }
        }

        for (new_task, target) in new_tasks {
            match self.tasks.entry(new_task) {
                hash_map::Entry::Occupied(mut entry) => entry.get_mut().target = target,
                hash_map::Entry::Vacant(entry) => { entry.insert(TaskData::new(target)); },
            }
        }
    }
}