#![recursion_limit="256"]
pub mod api;
pub mod channel;
pub mod context;
pub mod executor;
pub mod machine;
pub mod prelude;
pub mod spawner;
pub mod util;
use futures::task;
use futures::Future;
use std::pin::Pin;

pub fn machine<S: api::State<Machine = M>, M: api::Machine>(
    state: S,
) -> machine::LocalStateMachine<S::Machine> {
    machine::LocalStateMachine::new(state)
}

pub struct FutNever;
impl Future for FutNever {
    type Output = Never;

    fn poll(self: Pin<&mut Self>, _: &mut task::Context<'_>) -> task::Poll<Self::Output> {
        task::Poll::Pending
    }
}

pub enum Never {}
