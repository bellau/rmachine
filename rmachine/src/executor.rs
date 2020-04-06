use super::channel::oneshot;
use super::context::*;
use super::machine::MachineMessageEnvelope;
use crate::prelude::*;
use futures::future::Either;
use futures::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

use futures::future::FutureExt;
use futures::stream::StreamExt;

#[async_trait::async_trait(?Send)]
trait MachineStateExecutor<M: Machine> {
    async fn execute(self: Box<Self>) -> ExecutionResult<M, M::End>;
}

pub(crate) struct StateExecutor<S: State> {
    state: S,
    machine_context: Rc<RefCell<MachineExecutorContext>>,
}

impl<S: State> StateExecutor<S> {
    pub(crate) fn new(state: S, machine_context: Rc<RefCell<MachineExecutorContext>>) -> Self {
        StateExecutor::<S> { machine_context, state }
    }
}

type ExecutionResult<M, E> = Either<(Box<dyn MachineStateExecutor<M>>, M), E>;
type FnStateTransition<S, M> = Box<
    dyn FnOnce(Rc<RefCell<MachineExecutorContext>>, S) -> (Box<dyn MachineStateExecutor<M>>, M),
>;

#[async_trait::async_trait(?Send)]
impl<S: State> MachineStateExecutor<S::Machine> for StateExecutor<S> {
    async fn execute(self: Box<Self>) -> ExecutionResult<S::Machine, <S::Machine as Machine>::End> {
        let (end_tx, end_rx) = oneshot::channel();
        let end_rx = end_rx.fuse();
        let (transition_tx, transition_rx) = oneshot::channel();
        let transition_rx = transition_rx.fuse();

        let sec = RefCell::new(StateExecutorContext::new(
            self.machine_context.clone(),
            transition_tx,
            end_tx,
        ));

        let st = StateContext::new(sec);

        log::trace!(
            "run state {}::{}",
            std::any::type_name::<S::Machine>(),
            std::any::type_name::<S>()
        );

        use crate::util::select3 as select;
        use crate::util::SelectResult3::*;

        match select(self.state.run(&st), transition_rx, end_rx).await {
            B(Ok(transition_fun)) => {
                log::trace!(
                    "transtion state {}::{}",
                    std::any::type_name::<S::Machine>(),
                    std::any::type_name::<S>()
                );
                let mc = self.machine_context.clone();
                mc.borrow_mut().acceptors.reset();
                let res = transition_fun(mc, self.state);
                return Either::Left(res);
            }
            C(Ok(end_fun)) => {
                log::trace!(
                    "state {}::{} ended",
                    std::any::type_name::<S::Machine>(),
                    std::any::type_name::<S>()
                );

                return Either::Right(end_fun(self.state));
            }
            _ => {
                panic!("woop");
            }
        }
    }
}

impl<S: State> StateExecutorContext<S> {
    fn new(
        mcontext: Rc<RefCell<MachineExecutorContext>>,
        transition: channel::oneshot::Sender<FnStateTransition<S, S::Machine>>,
        end: channel::oneshot::Sender<Box<dyn FnOnce(S) -> <S::Machine as Machine>::End>>,
    ) -> Self {
        StateExecutorContext {
            machine_context: mcontext,
            transition: RefCell::new(Some(transition)),
            end: RefCell::new(Some(end)),
        }
    }
}
pub(crate) struct StateExecutorContext<S: State> {
    machine_context: Rc<RefCell<MachineExecutorContext>>,
    transition: RefCell<Option<channel::oneshot::Sender<FnStateTransition<S, S::Machine>>>>,
    end: RefCell<
        Option<channel::oneshot::Sender<Box<dyn FnOnce(S) -> <S::Machine as Machine>::End>>>,
    >,
}

impl<S: State> StateExecutorContext<S> {
    pub(crate) fn handle_transition<T, D>(&mut self, fun: T)
    where
        T: FnOnce(S) -> D,
        T: 'static,
        D: State<Machine = S::Machine>,
    {
        let fun = |ctx: Rc<RefCell<MachineExecutorContext>>, state: S| {
            let m = state.machine();
            let ns = fun(state);
            log::trace!(
                "transition from {}:{} to {}:{}",
                std::any::type_name::<S::Machine>(),
                std::any::type_name::<S>(),
                std::any::type_name::<D::Machine>(),
                std::any::type_name::<D>()
            );

            let me: Box<dyn MachineStateExecutor<S::Machine>> =
                Box::new(StateExecutor::new(ns, ctx));

            (me, m)
        };

        let _ = self.transition.borrow_mut().take().unwrap().send(Box::new(fun));
    }

    pub(crate) fn handle_end<T>(&mut self, fun: T)
    where
        T: FnOnce(S) -> <S::Machine as Machine>::End,
        T: 'static,
    {
        let _ = self.end.borrow_mut().take().unwrap().send(Box::new(fun));
    }

    pub(crate) fn handle_acceptor<I: 'static>(&mut self, tx: oneshot::Sender<I>) {
        self.machine_context.borrow_mut().handle_acceptor(tx);
    }

    pub(crate) fn handle_stream<St, I>(&mut self, stream: St)
    where
        S::Machine: InputHandler<I>,
        St: Stream<Item = I> + Unpin + 'static,
        I: 'static,
    {
        MachineExecutorContext::handle_stream(&self.machine_context, stream);
    }

    pub(crate) fn handle_future_input<F, I>(&mut self, fut: F)
    where
        S::Machine: InputHandler<I>,
        F: Future<Output = I> + Unpin + 'static,
        I: 'static,
    {
        MachineExecutorContext::handle_future_input(&self.machine_context, fut);
    }
}

pub(crate) struct MachineExecutorContext {
    pub(crate) acceptors: Acceptors,
}

impl MachineExecutorContext {
    pub(crate) fn new() -> MachineExecutorContext {
        MachineExecutorContext { acceptors: Acceptors::new() }
    }
    pub(crate) fn handle_acceptor<I: 'static>(&mut self, tx: oneshot::Sender<I>) {
        self.acceptors.handle_tx(tx);
    }

    pub(crate) fn hanle_input<I: 'static>(&mut self, i: I) {
        self.acceptors.handle_input(i);
    }

    pub(crate) fn handle_stream<St, I>(context: &Rc<RefCell<Self>>, mut stream: St)
    where
        St: Stream<Item = I> + Unpin + 'static,
        I: 'static,
    {
        let c = context.clone();
        let fut = async move {
            while let Some(i) = stream.next().await {
                log::trace!("stream of {} emit input", std::any::type_name::<I>());
                c.borrow_mut().hanle_input(i);
            }
            log::trace!("stream of {} finished", std::any::type_name::<I>());
        };

        crate::spawner::spawn(fut);
    }

    pub(crate) fn handle_future_input<F, I>(context: &Rc<RefCell<Self>>, fut: F)
    where
        F: Future<Output = I> + Unpin + 'static,
        I: 'static,
    {
        let c = context.clone();
        let fut = async move {
            let i = fut.await;
            log::trace!("future of {} emit input", std::any::type_name::<I>());
            c.borrow_mut().hanle_input(i);
            log::trace!("future of {} finished", std::any::type_name::<I>());
        };

        crate::spawner::spawn(fut);
    }
}

pub(crate) struct MachineExecutor;

impl MachineExecutor {
    pub(crate) async fn bootstrap<S: State>(
        mut machine_rx: crate::channel::mpsc::UnboundedReceiver<Box<dyn MachineMessageEnvelope>>,
        mut machine_transition_tx: crate::channel::watch::Sender<S::Machine>,
        end_tx: crate::channel::oneshot::Sender<<S::Machine as Machine>::End>,
        state: S,
    ) -> <S::Machine as Machine>::End {
        let machine_context = Rc::new(RefCell::new(MachineExecutorContext::new()));

        let m = state.machine();
        let _ = machine_transition_tx.try_send(m);

        let mut mse: Box<dyn MachineStateExecutor<S::Machine>> =
            Box::new(StateExecutor::new(state, machine_context.clone()));

        spawner::spawn(async move {
            while let Some(mut msg) = machine_rx.next().await {
                log::trace!("machine {} ready", std::any::type_name::<S::Machine>());
                msg.handle(&mut machine_context.borrow_mut());
            }
        });

        loop {
            match mse.execute().await {
                Either::Left(e) => {
                    let _ = machine_transition_tx.send(e.1).await;
                    mse = e.0;
                }
                Either::Right(end) => {
                    let _ = end_tx.send(end.clone());
                    return end;
                }
            }
        }
    }
}
