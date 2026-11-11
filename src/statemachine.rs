use std::{convert::Infallible, fmt::Display, ops::{ControlFlow, FromResidual, Residual, Try}};
use itertools::Itertools;
use log::{error, warn};

#[must_use]
pub enum Flow<T = (), E = anyhow::Error> {
    Continue(T),
    Break,
    Fail(E)
}

impl<T, E> Flow<T, E> {
    pub fn map_continue<U>(self, f: impl FnOnce(T) -> U) -> Flow<U, E> {
        match self {
            Flow::Continue(val) => Flow::Continue(f(val)),
            Flow::Break => Flow::Break,
            Flow::Fail(e) => Flow::Fail(e),
        }
    }
}

impl<T, E> Try for Flow<T, E> {
    type Output = T;
    type Residual = Flow<Infallible, E>;

    fn from_output(output: Self::Output) -> Self {
        Self::Continue(output)
    }

    fn branch(self) -> ControlFlow<Self::Residual, Self::Output> {
        match self {
            Flow::Continue(val) => ControlFlow::Continue(val),
            Flow::Break => ControlFlow::Break(Flow::Break),
            Flow::Fail(e) => ControlFlow::Break(Flow::Fail(e)),
        }
    }
}

impl<T, E, F: Into<E>> FromResidual<Flow<Infallible, F>> for Flow<T, E> {
    fn from_residual(residual: Flow<Infallible, F>) -> Self {
        match residual {
            Flow::Break => Self::Break,
            Flow::Fail(e) => Self::Fail(e.into()),
        }
    }
}

impl<T, E, F: Into<E>> FromResidual<Result<Infallible, F>> for Flow<T, E> {
    fn from_residual(Err(e): Result<Infallible, F>) -> Self {
        Self::Fail(e.into())
    }
}

impl<T, E, F: Into<E>> From<Result<T, F>> for Flow<T, E> {
    fn from(result: Result<T, F>) -> Self {
        match result {
            Ok(val) => Self::Continue(val),
            Err(e) => Self::Fail(e.into()),
        }
    }
}

/// Explicitly treat a missing value as "stop for this tick".
pub trait OrBreak<T> {
    fn or_break(self) -> Flow<T>;
}

impl<T> OrBreak<T> for Option<T> {
    fn or_break(self) -> Flow<T> {
        self.map_or(Flow::Break, Flow::Continue)
    }
}

impl<T, E> Residual<T> for Flow<Infallible, E> { type TryType = Flow<T, E>; }

const MAX_TRANSITIONS: u32 = 20;

#[expect(clippy::to_string_in_format_args)]
pub fn step<T, E, F>(state: &mut T, mut transition: F)
where
    F : FnMut(&mut T) -> Flow<T, E>,
    T : Default + Display,
    E : Display
{
    let mut state_names = vec![];

    for _ in 0..MAX_TRANSITIONS {
        let curr_state_name = state.to_string();

        match transition(state) {
            Flow::Fail(e) => {
                error!("Failed on state {curr_state_name}. Falling back to default state: {e}");
                *state = T::default();
                return;
            },
            Flow::Break => return,
            Flow::Continue(new_state) => {
                state_names.push(curr_state_name);
                *state = new_state;
            }
        }
    }

    state_names.push(state.to_string());
    warn!("Transitioned too many times:\n{}", state_names.into_iter().format(" -> ").to_string());
}

pub fn break_if(cond: bool) -> Flow {
    if cond { Flow::Break } else { Flow::Continue(()) }
}

/// `goto!(state)` transitions unconditionally.
/// `goto!(if <condition> { state })` transitions when the condition holds; the condition is anything
/// `if` accepts, including `let` patterns and let chains (`let Some(x) = y && x > 0`).
#[macro_export]
macro_rules! goto {
    (if $($rest:tt)+) => {
        $crate::goto!(@cond [] $($rest)+)
    };
    // A brace group as the very last token is the body. Rust forbids bare struct literals in `if`
    // conditions, so this is the same rule the compiler uses to find where the condition ends.
    (@cond [$($cond:tt)+] { $next:expr }) => {
        if $($cond)+ { return $crate::statemachine::Flow::Continue($next) }
    };
    (@cond [$($cond:tt)*] $head:tt $($rest:tt)*) => {
        $crate::goto!(@cond [$($cond)* $head] $($rest)*)
    };
    ($next:expr) => {
        return $crate::statemachine::Flow::Continue($next)
    };
}
