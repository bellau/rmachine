use crate::context::*;
use crate::executor::*;
use crate::{FutNever, Never};
use async_trait::async_trait;
use future::FusedFuture;
use futures::prelude::*;
use std::cell::RefCell;
use std::pin::Pin;

pub trait Machine: Sized + Clone + Send + 'static {
    type End: Clone;

    fn machine_id() -> &'static str {
        std::any::type_name::<Self>()
    }
}

#[async_trait(?Send)]
pub trait State: 'static {
    type Machine: Machine;

    async fn run(&self, context: &StateContext<Self>) -> Never
    where
        Self: Sized;

    fn machine(self: &Self) -> Self::Machine;

    fn state_id(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
}

pub trait InputHandler<I: 'static>: Machine {}

pub struct StateContext<S: State> {
    executor_context: RefCell<StateExecutorContext<S>>,
}

pub type RStream<I> = Pin<Box<dyn Stream<Item = I> + Unpin + 'static>>;

impl<S: State> StateContext<S> {
    pub(crate) fn new(executor_context: RefCell<StateExecutorContext<S>>) -> Self {
        StateContext { executor_context: executor_context }
    }
    pub fn accept<I>(&self) -> Box<dyn FusedFuture<Output = I> + Unpin>
    where
        S::Machine: InputHandler<I>,
        I: 'static,
    {
        let (tx, rx) = crate::channel::oneshot::channel();
        self.executor_context.borrow_mut().handle_acceptor(tx);
        Box::new(AcceptorFut::new(rx).fuse())
    }

    pub fn transition<T, D>(&self, fun: T) -> Pin<Box<dyn Future<Output = Never>>>
    where
        T: FnOnce(S) -> D,
        T: 'static,
        D: State<Machine = S::Machine>,
    {
        self.executor_context.borrow_mut().handle_transition(fun);

        futures::future::pending().boxed_local()
    }

    pub fn end<T>(&self, fun: T) -> FutNever
    where
        T: FnOnce(S) -> <S::Machine as Machine>::End,
        T: 'static,
    {
        self.executor_context.borrow_mut().handle_end(fun);
        FutNever
    }

    pub async fn never(&self) -> Never {
        futures::future::pending().await
    }

    pub fn stream_as_input<St, I>(&self, stream: St)
    where
        S::Machine: InputHandler<I>,
        St: Stream<Item = I> + Unpin + 'static,
        I: 'static,
    {
        self.executor_context.borrow_mut().handle_stream(stream);
    }

    pub fn as_input<F, I>(&self, fut: F)
    where
        S::Machine: InputHandler<I>,
        F: Future<Output = I> + Unpin + 'static,
        I: 'static,
    {
        self.executor_context.borrow_mut().handle_future_input(fut);
    }
}
