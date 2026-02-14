use std::fmt::Debug;
use log::*;

use crate::memory::Memory;

pub trait StateMachine<O> where Self : Sized {
    fn update(&self, underlying: &O, mem: &mut Memory) -> Result<Self, ()>;
}

pub fn transition<S, O>(state: &S, underlying: &O, mem: &mut Memory) -> S where S : StateMachine<O> + Default + Eq + Debug, O : Debug {
    transition_counted(state, underlying, mem, 0)
}

fn transition_counted<S, O>(state: &S, underlying: &O, mem: &mut Memory, transition_count: usize) -> S where S : StateMachine<O> + Default + Eq + Debug, O : Debug {
    let Ok(new_state) = state.update(underlying, mem) else {
        if *state == S::default() {
            error!("{underlying:?} failed on default state");
            return S::default()
        } else {
            error!("{underlying:?} failed on state {state:?}. Falling back to default state");
            return S::default() // TODO: This should probably execute the default state
        }
    };

    if new_state != *state {
        if transition_count <= 10 {
            transition_counted(&new_state, underlying, mem, transition_count + 1)
        } else {
            warn!("Stopped {underlying:?} prematurely. Transitioned too many times");
            new_state
        }
    } else {
        new_state
    }
}