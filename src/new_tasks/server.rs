use std::{collections::HashMap, hash::Hash};

use serde::{Deserialize, Serialize};

use crate::safeid::{TryFromUnchecked, TryCheck};

/*
Requirements:
- Should support arbitrary clients: creeps, spawns, rooms, logical clients
    - A client implements hashable and eq


Questions:
- Should a client be able to claim multiple simultaneous tasks?
- What is a task?
- 

Minimal set of assumptions:
- Clients are hashable IDs
- 

*/

#[derive(Serialize, Deserialize)]
struct TaskSpecification<Task, Meta> {
    work_required: u32,
    task: Task,
    meta: Meta
}

impl<Task: TryFromUnchecked, Meta: TryFromUnchecked> TryFromUnchecked for TaskSpecification<Task, Meta>
{
    type Unchecked = TaskSpecification<Task::Unchecked, Meta::Unchecked>;

    fn try_from_unchecked(us: Self::Unchecked) -> Option<Self> {
        Some(Self {
            work_required: us.work_required,
            task: us.task.try_check()?,
            meta: us.meta.try_check()?,
        })
    }
}

#[derive(Serialize, Deserialize)]
struct TaskLease<Task> {
    pending: u32,
    last_heartbeat: u32,
    task: Task
}