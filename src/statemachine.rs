use std::fmt::Debug;
use log::{error, warn};
use screeps::{Creep, RoomName, SharedCreepProperties};

use crate::memory::Memory;

pub trait UnderlyingName { fn name(&self) -> String; }
impl UnderlyingName for Creep { fn name(&self) -> String { SharedCreepProperties::name(self) } }
impl UnderlyingName for RoomName { fn name(&self) -> String { self.to_string() } }

pub enum Transition<S> {
    Break(S),
    Continue(S),
    Stay
}

pub trait StateMachine<O> where Self : Sized {
    fn update(&self, underlying: &O, mem: &mut Memory) -> Result<Transition<Self>, ()>;
}

pub fn transition<S, O>(state: &S, underlying: &O, mem: &mut Memory) -> S where S : StateMachine<O> + Default + Eq + Debug + Clone, O : UnderlyingName {
    transition_counted(state, underlying, mem, 0)
}

fn transition_counted<S, O>(state: &S, underlying: &O, mem: &mut Memory, transition_count: usize) -> S where S : StateMachine<O> + Default + Eq + Debug + Clone, O : UnderlyingName {
    let Ok(transition) = state.update(underlying, mem) else {
        if *state == S::default() {
            error!("{} failed on default state", underlying.name());
            return S::default()
        }

        error!("{} failed on state {state:?}. Falling back to default state", underlying.name());
        return S::default() // TODO: This should probably execute the default state
    };

    match transition {
        Transition::Stay => state.clone(),
        Transition::Break(new_state) => new_state,
        Transition::Continue(new_state) => {
            if transition_count <= 10 {
                transition_counted(&new_state, underlying, mem, transition_count + 1)
            } else {
                warn!("Stopped {} prematurely. Transitioned too many times", underlying.name());
                new_state
            }
        },
    }
}