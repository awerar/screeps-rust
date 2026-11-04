use std::{convert::Infallible, fmt::Display, ops::{ControlFlow, FromResidual, Residual, Try}};
use itertools::Itertools;
use log::{error, warn};

#[must_use]
pub enum Step<S> {
    Next(S),
    Stay,
    Fail(anyhow::Error)
}

const MAX_TRANSITIONS: u32 = 20;

#[expect(clippy::to_string_in_format_args)]
pub fn step<T, F>(state: &mut T, mut transition: F)
where
    F : FnMut(&mut T) -> Step<T>,
    T : Default + Display
{
    let mut state_names = vec![];

    for _ in 0..MAX_TRANSITIONS {
        let curr_state_name = state.to_string();

        match transition(state) {
            Step::Fail(e) => {
                error!("Failed on state {curr_state_name}. Falling back to default state: {e}");
                *state = T::default();
                return;
            },
            Step::Stay => return,
            Step::Next(new_state) => {
                state_names.push(curr_state_name);
                *state = new_state;
            }
        }
    }

    state_names.push(state.to_string());
    warn!("Transitioned too many times:\n{}", state_names.into_iter().format(" -> ").to_string());
}

impl<S> FromResidual<Step<Infallible>> for Step<S> {
    fn from_residual(residual: Step<Infallible>) -> Self {
        match residual {
            Step::Stay => Self::Stay,
            Step::Fail(e) => Self::Fail(e),
            Step::Next(never) => match never {}
        }
    }
}

impl<S> FromResidual<Result<Infallible, anyhow::Error>> for Step<S> {
    fn from_residual(Err(e): Result<Infallible, anyhow::Error>) -> Self {
        Self::Fail(e)
    }
}

#[must_use]
pub enum Wait {
    Ready,
    NotYet,
    Fail(anyhow::Error)
}

impl Try for Wait {
    type Output = ();
    type Residual = Step<Infallible>;

    fn from_output((): ()) -> Self { Self::Ready }

    fn branch(self) -> ControlFlow<Step<Infallible>, ()> {
        match self {
            Self::Ready => ControlFlow::Continue(()),
            Self::NotYet => ControlFlow::Break(Step::Stay),
            Self::Fail(e) => ControlFlow::Break(Step::Fail(e))
        }
    }
}

impl FromResidual<Step<Infallible>> for Wait {
    fn from_residual(residual: Step<Infallible>) -> Self {
        match residual {
            Step::Stay => Self::NotYet,
            Step::Fail(e) => Self::Fail(e),
            Step::Next(never) => match never {}
        }
    }
}

impl Residual<()> for Step<Infallible> { type TryType = Wait; }

pub fn wait_if(cond: bool) -> Wait {
    if cond { Wait::NotYet } else { Wait::Ready }
}

#[macro_export]
macro_rules! next {
    ($next:expr) => {
        return $crate::statemachine::Step::Next($next)
    };
}

#[macro_export]
macro_rules! next_if {
    ($expr:expr, $next:expr) => {
        if $expr { $crate::next!($next) }
    };
}
