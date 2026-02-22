use std::{fmt::Display, mem};
use itertools::Itertools;
use log::{error, warn};
use screeps::{Creep, RoomName, SharedCreepProperties};

pub trait UnderlyingName { fn name(&self) -> String; }
impl UnderlyingName for Creep { fn name(&self) -> String { SharedCreepProperties::name(self) } }
impl UnderlyingName for RoomName { fn name(&self) -> String { self.to_string() } }

pub enum Transition<S> {
    Break(S),
    Continue(S)
}

pub trait StateMachine<U, D, S> where Self : Default + Display {
    fn update(self, underlying: &U, data: &D, systems: &mut S) -> Result<Transition<Self>, ()>;
}

pub trait StateMachineTransition<U, D, S> {
    fn transition(&mut self, underlying: &U, data: &D, systems: &mut S);
}

const MAX_TRANSITIONS: u32 = 10;
impl<SM, U : UnderlyingName, D, S> StateMachineTransition<U, D, S> for SM where SM : StateMachine<U, D, S> {
    fn transition(&mut self, underlying: &U, data: &D, systems: &mut S) {
        let mut state_names = vec![self.to_string()];

        for i in 0..MAX_TRANSITIONS {
            let curr_state_name = self.to_string();

            match mem::take(self).update(underlying, data, systems) {
                Err(()) => {
                    error!("{} failed on state {curr_state_name}. Falling back to default state", underlying.name());
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