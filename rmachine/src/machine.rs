use super::api::*;
use super::prelude::*;
use crate::executor::*;
use futures::future::FutureExt;
use futures::{SinkExt, StreamExt};
use log;
use std::pin::Pin;

pub(crate) trait MachineMessageEnvelope: std::fmt::Debug {
    fn handle(self: &mut Self, ctx: &mut MachineExecutorContext);
}

pub struct SyncStateMachine<M: Machine> {
    msg_sender: futures::channel::mpsc::UnboundedSender<Box<dyn MachineMessageEnvelope + Send>>,
    machine_rx: async_std::sync::Receiver<M>,
}

impl<M: Machine + 'static> SyncStateMachine<M> {
    fn new(st: &LocalStateMachine<M>) -> Self {
        Self::create(st)
    }

    fn create(st: &LocalStateMachine<M>) -> Self {
        let mut machine_rx = st.machine_rx.clone();
        let mut lmsg_sender = st.msg_sender.clone();
        let (tx, mut rx) =
            futures::channel::mpsc::unbounded::<Box<dyn MachineMessageEnvelope + Send>>();

        let (otx, orx) = async_std::sync::channel(20);

        let fut = async move {
            loop {
                let res = rx.next().await;
                if let Some(i) = res {
                    match lmsg_sender.send(i).await {
                        Err(_) => break,
                        _ => {}
                    }
                } else {
                    break;
                }
            }
        };
        let _ = spawner::spawn(fut);
        let _ = spawner::spawn(async move {
            while let Some(i) = machine_rx.next().await {
                otx.send(i).await;
            }
        });
        SyncStateMachine { msg_sender: tx, machine_rx: orx }
    }

    pub fn sync(&self) -> SyncStateMachine<M> {
        SyncStateMachine {
            msg_sender: self.msg_sender.clone(),
            machine_rx: self.machine_rx.clone(),
        }
    }
}

impl<M: Machine + 'static> SyncStateMachineController<M> for SyncStateMachine<M> {
    fn machine(&self) -> RStream<M> {
        Box::pin(self.machine_rx.clone())
    }

    fn input<I: 'static + Send>(&self, i: I)
    where
        M: InputHandler<I>,
    {
        let msg = InputMessageEnvelope { input: Some(i) };
        let _ = self.msg_sender.unbounded_send(Box::new(msg));
    }

    fn end(&self) -> Pin<Box<dyn Future<Output = M::End>>> {
        async { futures::future::pending().await }.boxed_local()
    }
}

pub struct LocalStateMachine<M: Machine> {
    msg_sender: crate::channel::mpsc::UnboundedSender<Box<dyn MachineMessageEnvelope>>,
    machine_rx: crate::channel::watch::Receiver<M>,
    end_rx: crate::channel::oneshot::Receiver<M::End>,
}

impl<M: Machine> LocalStateMachineController<M> for LocalStateMachine<M> {
    fn machine(&self) -> RStream<M> {
        Box::pin(self.machine_rx.clone())
    }

    fn input<I: 'static>(&self, i: I)
    where
        M: InputHandler<I>,
    {
        if self.msg_sender.is_closed() {
            log::warn!("machine is endend, no more message accepted");
            return;
        }

        let msg = InputMessageEnvelope { input: Some(i) };
        let _ = self.msg_sender.unbounded_send(Box::new(msg));
    }

    fn end(&self) -> Pin<Box<dyn Future<Output = M::End>>> {
        let end_rx = self.end_rx.clone();
        async {
            if let Ok(r) = end_rx.await {
                r
            } else {
                futures::future::pending().await
            }
        }
        .boxed_local()
    }
}

impl<M: Machine> LocalStateMachine<M> {
    pub fn new<S: State<Machine = M>>(state: S) -> LocalStateMachine<M> {
        let (tx, rx) = crate::channel::mpsc::unbounded();
        let (mtx, mrx) = crate::channel::watch::channel();
        let (etx, erx) = crate::channel::oneshot::channel();
        spawner::spawn(async move {
            MachineExecutor::bootstrap(rx, mtx, etx, state).await;
        });

        let ret = LocalStateMachine { msg_sender: tx, machine_rx: mrx, end_rx: erx };

        ret
    }

    pub fn reference(&self) -> LocalStateMachine<M> {
        self.clone()
    }

    pub fn sync(&self) -> SyncStateMachine<M> {
        SyncStateMachine::new(self)
    }
}

impl<M: Machine> std::clone::Clone for LocalStateMachine<M> {
    fn clone(&self) -> Self {
        LocalStateMachine {
            end_rx: self.end_rx.clone(),
            machine_rx: self.machine_rx.clone(),
            msg_sender: self.msg_sender.clone(),
        }
    }
}
struct InputMessageEnvelope<I: 'static> {
    input: Option<I>,
}

impl<I: 'static> MachineMessageEnvelope for InputMessageEnvelope<I> {
    fn handle(&mut self, ctx: &mut MachineExecutorContext) {
        ctx.hanle_input(self.input.take().unwrap());
    }
}

impl<I: 'static> std::fmt::Debug for InputMessageEnvelope<I> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        f.debug_struct("Inpuy").finish()
    }
}

use std::fmt::Formatter;

use std::fmt::Result;

pub trait SyncStateMachineController<M: Machine>: 'static {
    fn machine(&self) -> RStream<M>;

    fn input<I: 'static + Send>(&self, i: I)
    where
        M: InputHandler<I>;

    fn end(&self) -> Pin<Box<dyn Future<Output = M::End>>>;
}

pub trait LocalStateMachineController<M: Machine>: 'static {
    fn machine(&self) -> RStream<M>;

    fn input<I: 'static>(&self, i: I)
    where
        M: InputHandler<I>;

    fn end(&self) -> Pin<Box<dyn Future<Output = M::End>>>;
}
