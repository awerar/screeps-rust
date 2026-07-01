use std::{collections::HashMap, hash::Hash, marker::PhantomData};

use derive_where::derive_where;
use screeps::game;

use crate::check::CheckFrom;

pub trait TaskKind {
    type Task: Hash + Eq;
    type Client: Hash + Eq + CheckFrom;
    type TaskState;
    type ClientState;
}

pub enum ClientFailure<Err> {
    Timeout,
    Invalid(Err)
}

pub trait TaskState<K: TaskKind<TaskState = Self>> {
    fn complete(&mut self, client: &K::Client, cstate: K::ClientState);
    fn fail(&mut self, failure: ClientFailure<<K::Client as CheckFrom>::Err>, cstate: K::ClientState);
}

struct ClientRecord<K: TaskKind> {
    state: K::ClientState,
    last_heartbeat: u32
}

#[derive_where(Serialize, Deserialize; K::TaskState, HashMap<K::Client, ClientRecord<K>>)]
struct TaskRecord<K: TaskKind> {
    state: K::TaskState,
    clients: HashMap<K::Client, ClientRecord<K>>,
}

pub trait TaskPolicy<K: TaskKind> {
    type Score;

    fn score(task: &K::Task, client: &K::Client, state: K::ClientState, cstate: &K::ClientState) -> Self::Score;
}

struct TaskServer<K: TaskKind, Policy, const TIMEOUT: u32 = 5> {
    tasks: HashMap<K::Task, TaskRecord<K>>,
    phantom: PhantomData<Policy>
}

impl<K: TaskKind, Policy, const TIMEOUT: u32> TaskServer<K, Policy, TIMEOUT>
where 
    K::TaskState: TaskState<K>,
    Policy: TaskPolicy<K>
{
    pub fn new() -> Self {
        Self { tasks: HashMap::new(), phantom: PhantomData }
    }

    pub fn heartbeat(&mut self, client: &K::Client, task: &K::Task) -> Result<(), ()> {
        let Some(task) = self.tasks.get_mut(task) else { return Err(()); };
        let Some(client_record) = task.clients.get_mut(client) else { return Err(()) };

        if game::time() > client_record.last_heartbeat + TIMEOUT {
            let client = task.clients.remove(client).unwrap();

            task.state.fail(ClientFailure::Timeout, client.state);
            Err(())
        } else {
            client_record.last_heartbeat = game::time();
            Ok(())
        }
    }
}