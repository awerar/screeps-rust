use std::{fmt::Display, mem};
use itertools::Itertools;
use log::{error, warn};
use screeps::{Creep, Room, SharedCreepProperties};

pub trait UnderlyingName { fn name(&self) -> String; }
impl UnderlyingName for Creep { fn name(&self) -> String { SharedCreepProperties::name(self) } }
impl UnderlyingName for Room { fn name(&self) -> String { self.name().to_string() } }

pub enum Transition<S> {
    Break(S),
    Continue(S)
}

pub trait StateMachine<U, A> where Self : Default + Display {
    fn update(self, underlying: &U, args: &mut A) -> anyhow::Result<Transition<Self>>;
}

pub trait StateMachineTransition<U, A> {
    fn transition(&mut self, underlying: &U, args: &mut A);
}

const MAX_TRANSITIONS: u32 = 10;
impl<SM, U : UnderlyingName, A> StateMachineTransition<U, A> for SM where SM : StateMachine<U, A> {
    fn transition(&mut self, underlying: &U, args: &mut A) {
        let mut state_names = vec![self.to_string()];

        for _ in 0..MAX_TRANSITIONS {
            let curr_state_name = self.to_string();

            match mem::take(self).update(underlying, args) {
                Err(e) => {
                    error!("{} failed on state {curr_state_name}. Falling back to default state: {e}", underlying.name());
                    return;
                },
                Ok(Transition::Break(new_state)) => {
                    *self = new_state;
                    return;
                },
                Ok(Transition::Continue(new_state)) => {
                    state_names.push(curr_state_name);
                    *self = new_state;
                }
            }
        }

        warn!("Stopped {} prematurely. Transitioned too many times:\n{}", underlying.name(), state_names.into_iter().format(" -> "));
    }
}