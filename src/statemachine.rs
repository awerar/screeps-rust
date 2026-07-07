use std::{fmt::Display, mem};
use itertools::Itertools;
use log::{error, warn};

pub enum Transition<S> {
    Done(S),
    Next(S)
}

const MAX_TRANSITIONS: u32 = 20;

#[expect(clippy::to_string_in_format_args)]
pub fn run_transitions<T, F>(mut state: T, mut transition: F) -> T 
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
            Ok(Transition::Done(state)) => {
                return state;
            },
            Ok(Transition::Next(new_state)) => {
                state_names.push(curr_state_name);
                state = new_state;
            }
        }
    }

    state_names.push(state.to_string());
    warn!("Transitioned too many times:\n{}", state_names.into_iter().format(" -> ").to_string());
    state
}

pub fn step<T, F>(state: &mut T, transition: F)
where
    F : FnMut(T) -> anyhow::Result<Transition<T>>,
    T : Default + Display
{
    *state = run_transitions(mem::take(state), transition);
}

#[macro_export]
macro_rules! done {
    ($next:expr) => {
        return std::result::Result::Ok($crate::statemachine::Transition::Done($next))
    };
}

#[macro_export]
macro_rules! done_if {
    ($expr:expr, $next:expr) => {
        if $expr { $crate::done!($next) }
    };
}

#[macro_export]
macro_rules! next {
    ($next:expr) => {
        return std::result::Result::Ok($crate::statemachine::Transition::Next($next))
    };
}

#[macro_export]
macro_rules! next_if {
    ($expr:expr, $next:expr) => {
        if $expr { $crate::next!($next) }
    };
}

pub trait ShouldYield {
    fn should_yield(&self) -> bool;
}

impl<T: ShouldYield, E: ShouldYield> ShouldYield for Result<T, E> {
    fn should_yield(&self) -> bool {
        match self {
            Ok(val) => val.should_yield(),
            Err(err) => err.should_yield(),
        }
    }
}

impl ShouldYield for anyhow::Error { fn should_yield(&self) -> bool { false } }

#[macro_export]
macro_rules! defer {
    ($expr:expr, $next:expr) => {
        match $expr {
            val if $crate::statemachine::ShouldYield::should_yield(&val) => $crate::done!($next),
            val => val
        }
    };
}

#[macro_export]
macro_rules! defer_err {
    ($expr:expr, $next:expr) => {
        match $expr {
            std::result::Result::Ok(val) => std::result::Result::Ok(val),
            std::result::Result::Err(e) => std::result::Result::Err($crate::defer!(e, $next))
        }
    };
}