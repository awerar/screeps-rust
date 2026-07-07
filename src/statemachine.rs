use std::{fmt::Display, mem};
use itertools::Itertools;
use log::{error, warn};

pub enum Transition<S> {
    Break(S),
    Continue(S)
}

const MAX_TRANSITIONS: u32 = 20;

#[expect(clippy::to_string_in_format_args)]
pub fn update_many<T, F>(mut state: T, mut transition: F) -> T 
where
    F : FnMut(T) -> anyhow::Result<Transition<T>>,
    T : Default + Display
{
    let mut state_names = vec![];

    for _ in 0..MAX_TRANSITIONS {
        let curr_state_name = state.to_string();

        match transition(state) {
            Err(e) => {
                error!("Failed on state {curr_state_name}. Falling back to default state: {e}");
                return T::default();
            },
            Ok(Transition::Break(state)) => {
                return state;
            },
            Ok(Transition::Continue(new_state)) => {
                state_names.push(curr_state_name);
                state = new_state;
            }
        }
    }

    state_names.push(state.to_string());
    warn!("Transitioned too many times:\n{}", state_names.into_iter().format(" -> ").to_string());
    state
}

pub fn transition<T, F>(state: &mut T, transition: F)
where
    F : FnMut(T) -> anyhow::Result<Transition<T>>,
    T : Default + Display
{
    *state = update_many(mem::take(state), transition);
}

#[macro_export]
macro_rules! brk {
    ($next:expr) => {
        return std::result::Result::Ok($crate::statemachine::Transition::Break($next))
    };
}

#[macro_export]
macro_rules! break_if {
    ($expr:expr, $next:expr) => {
        if $expr { $crate::brk!($next) }
    };
}

#[macro_export]
macro_rules! cont {
    ($next:expr) => {
        return std::result::Result::Ok($crate::statemachine::Transition::Continue($next))
    };
}

#[macro_export]
macro_rules! continue_if {
    ($expr:expr, $next:expr) => {
        if $expr { $crate::cont!($next) }
    };
}