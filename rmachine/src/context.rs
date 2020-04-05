use super::api::*;
use futures::prelude::*;
use futures::task;
use futures::task::Poll;
use std::any::Any;
use std::collections::VecDeque;
use std::pin::Pin;

use super::channel::oneshot;

struct Acceptor<I> {
    sender: Option<oneshot::Sender<I>>,
    inputs: VecDeque<I>,
}

struct AcceptorHandler {
    reset_func: Box<dyn Fn(&mut Box<dyn Any>) -> ()>,
    acceptor: Box<dyn Any>,
}

impl AcceptorHandler {
    fn new<I: 'static>() -> AcceptorHandler {
        let a = Acceptor::<I>::new();
        let fun = move |a: &mut Box<dyn Any>| {
            a.downcast_mut::<Acceptor<I>>().unwrap().reset();
        };

        AcceptorHandler { reset_func: Box::new(fun), acceptor: Box::new(a) }
    }

    fn can<I: 'static>(&self) -> bool {
        self.acceptor.is::<Acceptor<I>>()
    }

    fn acceptor<I: 'static>(&mut self) -> Option<&mut Acceptor<I>> {
        self.acceptor.downcast_mut::<Acceptor<I>>()
    }

    fn reset(&mut self) {
        (self.reset_func)(&mut self.acceptor);
    }
}

impl<I: 'static> Acceptor<I> {
    fn new() -> Self {
        Acceptor { sender: None, inputs: VecDeque::new() }
    }

    fn handle_tx(&mut self, tx: oneshot::Sender<I>) {
        self.reset();
        if let Some(input) = self.inputs.pop_front() {
            if let Err(value) = tx.send(input) {
                log::trace!("acceptor found for {} sending data", std::any::type_name::<I>());
                self.inputs.push_front(value);
            } else {
                log::trace!(
                    "acceptor for {} but error sending data to new TX",
                    std::any::type_name::<I>()
                );
            }
        } else {
            log::trace!("acceptor for {} but no data available", std::any::type_name::<I>());
            self.sender = Some(tx);
        }
    }

    fn handle_input(&mut self, i: I) {
        if let Some(last_sender) = self.sender.take() {
            if let Err(value) = last_sender.send(i) {
                log::trace!(
                    "acceptor found for {} but error occur sending data",
                    std::any::type_name::<I>()
                );
                self.inputs.push_back(value);
            } else {
                log::trace!("acceptor for {} and data sended", std::any::type_name::<I>());
            }
        } else {
            log::trace!("no acceptor for {}", std::any::type_name::<I>());
            self.inputs.push_back(i);
        }
    }

    fn reset(&mut self) {
        if let Some(last_sender) = self.sender.take() {
            log::trace!("reset acceptor for {}", std::any::type_name::<I>());

            if let Some(value) = last_sender.value() {
                log::trace!("reset acceptor for {} and data available", std::any::type_name::<I>());
                self.inputs.push_front(value);
            } else {
                log::trace!(
                    "reset acceptor for {} but no data available",
                    std::any::type_name::<I>()
                );
            }
        } else {
            log::trace!(
                "reset acceptor for {}, no tx found (vals:{})",
                std::any::type_name::<I>(),
                self.inputs.len()
            );
        }
    }
}

struct Outputer<O: Output> {
    output: Option<O>,
    senders: Vec<crate::channel::watch::Sender<O>>,
}

impl<O: Output> Outputer<O> {
    fn handle_output(&mut self, o: O) {
        self.senders.iter_mut().for_each(|s| {
            let _ = s.start_send(o.clone());
        });

        self.output = Some(o);

        self.senders.retain(|s| !s.is_closed());
    }

    fn handle_tx(&mut self, mut tx: crate::channel::watch::Sender<O>) {
        if let Some(o) = self.output.as_ref() {
            let _ = tx.start_send(o.clone());
        }
        self.senders.push(tx);
        self.senders.retain(|s| !s.is_closed());
    }
    fn new() -> Self {
        Outputer { output: None, senders: vec![] }
    }
}
struct OutputHandler {
    outputer: Box<dyn Any>,
}

impl OutputHandler {
    fn new<O: Output>() -> Self {
        let a = Outputer::<O>::new();
        OutputHandler { outputer: Box::new(a) }
    }

    fn can<O: Output>(&self) -> bool {
        self.outputer.is::<Outputer<O>>()
    }

    fn outputer<O: Output>(&mut self) -> Option<&mut Outputer<O>> {
        self.outputer.downcast_mut::<Outputer<O>>()
    }
}

pub(crate) struct Outputers {
    outputers: Vec<OutputHandler>,
}

impl Outputers {
    pub(crate) fn new() -> Self {
        Outputers { outputers: vec![] }
    }

    pub(crate) fn handle_output<O: Output>(&mut self, o: O)
    where
        Self: Sized,
    {
        self.outputer().handle_output(o);
    }

    pub(crate) fn handle_tx<O: Output>(&mut self, tx: crate::channel::watch::Sender<O>)
    where
        Self: Sized,
    {
        self.outputer().handle_tx(tx);
    }

    fn outputer<O: Output>(&mut self) -> &mut Outputer<O> {
        if let Some(pos) = self.outputers.iter().position(|a| a.can::<O>()) {
            return self.outputers.get_mut(pos).unwrap().outputer::<O>().unwrap();
        }

        let outputer = OutputHandler::new::<O>();
        self.outputers.push(outputer);
        self.outputers.last_mut().unwrap().outputer::<O>().unwrap()
    }
}

pub(crate) struct Acceptors {
    acceptors: Vec<AcceptorHandler>,
}

impl Acceptors {
    pub(crate) fn new() -> Acceptors {
        Acceptors { acceptors: vec![] }
    }
    pub(crate) fn reset(&mut self) {
        self.acceptors.iter_mut().for_each(|a| {
            a.reset();
        })
    }
    pub(crate) fn handle_input<I: 'static>(&mut self, i: I)
    where
        Self: Sized,
    {
        self.acceptor().handle_input(i);
    }

    pub(crate) fn handle_tx<I: 'static>(&mut self, tx: oneshot::Sender<I>)
    where
        Self: Sized,
    {
        self.acceptor().handle_tx(tx);
    }

    fn acceptor<I: 'static>(&mut self) -> &mut Acceptor<I> {
        if let Some(pos) = self.acceptors.iter().position(|a| a.can::<I>()) {
            return self.acceptors.get_mut(pos).unwrap().acceptor::<I>().unwrap();
        }

        log::trace!("no acceptor found for {}, create new one", std::any::type_name::<I>());
        let acceptor = AcceptorHandler::new::<I>();
        self.acceptors.push(acceptor);
        self.acceptors.last_mut().unwrap().acceptor::<I>().unwrap()
    }
}

pub(crate) struct AcceptorFut<I: 'static> {
    response: Option<oneshot::Receiver<I>>,
}

impl<I: 'static> AcceptorFut<I> {
    pub(crate) fn new(rx: oneshot::Receiver<I>) -> Self {
        AcceptorFut { response: Some(rx) }
    }
}
impl<I: 'static> Future for AcceptorFut<I> {
    type Output = I;
    fn poll(self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> Poll<Self::Output> {
        // FIXME get_mut
        let s = unsafe { self.get_unchecked_mut() };
        let mut recv = s.response.take().expect("task finisehd");
        match Pin::new(&mut recv).poll(cx) {
            Poll::Ready(Ok(res)) => {
                return Poll::Ready(res);
            }
            _ => {
                s.response = Some(recv);
                return Poll::Pending;
            }
        }
    }
}
