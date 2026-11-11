use std::{convert::Infallible, fmt::Display, ops::{ControlFlow, FromResidual, Residual, Try}};
use anyhow::anyhow;
use itertools::Itertools;

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

pub trait OrBreak<T> {
    fn or_break(self) -> Flow<T>;
}

impl<T> OrBreak<T> for Option<T> {
    fn or_break(self) -> Flow<T> {
        self.map_or(Flow::Break, Flow::Continue)
    }
}

impl<T, E> Residual<T> for Flow<Infallible, E> { type TryType = Flow<T, E>; }

pub enum Next<S> {
    Goto(S),
    Stay(S),
}

impl<S> Next<S> {
    pub fn map<U>(self, f: impl FnOnce(S) -> U) -> Next<U> {
        match self {
            Next::Goto(state) => Next::Goto(f(state)),
            Next::Stay(state) => Next::Stay(f(state)),
        }
    }
}

const MAX_TRANSITIONS: u32 = 20;

#[expect(clippy::to_string_in_format_args)]
pub fn step<T, E, F>(mut state: T, mut transition: F) -> Result<T, anyhow::Error>
where
    F : FnMut(T) -> Flow<Next<T>, E>,
    T : Display + Clone,
    E : Into<anyhow::Error>
{
    let mut state_names = vec![];

    for _ in 0..MAX_TRANSITIONS {
        match transition(state.clone()) {
            Flow::Fail(e) => return Err(e.into().context(format!("Failed in state {state}"))),
            Flow::Break => return Ok(state),
            Flow::Continue(Next::Stay(new_state)) => return Ok(new_state),
            Flow::Continue(Next::Goto(new_state)) => {
                state_names.push(state.to_string());
                state = new_state;
            }
        }
    }

    state_names.push(state.to_string());
    Err(anyhow!("Transitioned too many times:\n{}", state_names.into_iter().format(" -> ").to_string()))
}

pub fn break_if(cond: bool) -> Flow {
    if cond { Flow::Break } else { Flow::Continue(()) }
}

/// `goto!(state)` transitions unconditionally and runs the new state's transition this tick.
/// `goto!(if <condition> { state })` transitions when the condition holds; the condition is anything
/// `if` accepts, including `let` patterns and let chains (`let Some(x) = y && x > 0`).
#[macro_export]
macro_rules! goto {
    ($($args:tt)+) => { $crate::__return_next!(Goto; $($args)+) };
}

/// `stay!(state)` stops for this tick, keeping `state`.
/// `stay!(if <condition> { state })` does so when the condition holds, with the same rules as `goto!`.
#[macro_export]
macro_rules! stay {
    ($($args:tt)+) => { $crate::__return_next!(Stay; $($args)+) };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __return_next {
    ($variant:ident; if $($rest:tt)+) => {
        $crate::__return_next!(@cond $variant [] $($rest)+)
    };
    // A brace group as the very last token is the body. Rust forbids bare struct literals in `if`
    // conditions, so this is the same rule the compiler uses to find where the condition ends.
    (@cond $variant:ident [$($cond:tt)+] { $next:expr }) => {
        if $($cond)+ { return $crate::statemachine::Flow::Continue($crate::statemachine::Next::$variant($next)) }
    };
    (@cond $variant:ident [$($cond:tt)*] $head:tt $($rest:tt)*) => {
        $crate::__return_next!(@cond $variant [$($cond)* $head] $($rest)*)
    };
    ($variant:ident; $next:expr) => {
        return $crate::statemachine::Flow::Continue($crate::statemachine::Next::$variant($next))
    };
}
