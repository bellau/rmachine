use super::atomic_waker::AtomicWaker;
use core::fmt;
use core::pin::Pin;
use futures::stream::FusedStream;
use futures::stream::Stream;
use futures::task::{Context, Poll};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::AtomicUsize;
#[derive(Debug)]
struct SenderInner<T: Clone> {
    inner: Rc<Inner<T>>,
}

impl<T: Clone> Unpin for Receiver<T> {}
impl<T: Clone> Unpin for Sender<T> {}

impl<T: Clone> Unpin for SenderInner<T> {}

#[derive(Debug)]
pub struct Sender<T: Clone>(Option<SenderInner<T>>);

#[derive(Debug)]
pub struct Receiver<T: Clone> {
    inner: Option<Rc<Inner<T>>>,
    waker: Rc<AtomicWaker>,
    version: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SendError {
    kind: SendErrorKind,
}

#[derive(Clone, PartialEq, Eq)]
pub struct TrySendError<T> {
    err: SendError,
    val: T,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum SendErrorKind {
    Disconnected,
}

pub struct TryRecvError {
    _priv: (),
}

impl fmt::Display for SendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "send failed because receiver is gone")
    }
}

impl std::error::Error for SendError {}

impl SendError {
    pub fn is_disconnected(&self) -> bool {
        true
    }
}

impl<T> fmt::Debug for TrySendError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TrySendError").field("kind", &self.err.kind).finish()
    }
}

impl<T> fmt::Display for TrySendError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "send failed because receiver is gone")
    }
}

impl<T: core::any::Any> std::error::Error for TrySendError<T> {}

impl<T> TrySendError<T> {
    pub fn is_disconnected(&self) -> bool {
        self.err.is_disconnected()
    }

    pub fn into_inner(self) -> T {
        self.val
    }

    pub fn into_send_error(self) -> SendError {
        self.err
    }
}

impl fmt::Debug for TryRecvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("TryRecvError").finish()
    }
}

impl fmt::Display for TryRecvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "receiver channel is empty")
    }
}

impl std::error::Error for TryRecvError {}

#[derive(Debug, Clone)]
struct Value<T: Clone> {
    version: u32,
    value: T,
}

impl<T: Clone> Value<T> {
    fn new(old_value: Option<Value<T>>, value: T) -> Self {
        Value { version: old_value.map_or(1, |v| v.version + 1), value }
    }
}
#[derive(Debug)]
struct Inner<T: Clone> {
    state: RefCell<InnerState>,
    message: RefCell<Option<Value<T>>>,
    receivers: RefCell<Vec<Rc<AtomicWaker>>>,
    num_receivers: AtomicUsize,
}

#[derive(Debug)]
enum InnerState {
    Init,
    Closed,
}

impl InnerState {
    fn is_open(&self) -> bool {
        if let &InnerState::Init = self {
            true
        } else {
            false
        }
    }

    fn close(&mut self) {
        *self = Self::Closed;
    }
}

pub fn channel<T: Clone>() -> (Sender<T>, Receiver<T>) {
    let (tx, rx) = channel2();
    (Sender(Some(tx)), rx)
}

fn channel2<T: Clone>() -> (SenderInner<T>, Receiver<T>) {
    let rx_waker = Rc::new(AtomicWaker::new());
    let inner = Rc::new(Inner {
        state: RefCell::new(InnerState::Init),
        message: RefCell::new(None),
        receivers: RefCell::new(vec![rx_waker.clone()]),
        num_receivers: AtomicUsize::new(1),
    });

    let tx = SenderInner { inner: inner.clone() };

    let rx = Receiver { inner: Some(inner), waker: rx_waker, version: 0 };

    (tx, rx)
}

impl<T: Clone> SenderInner<T> {
    fn try_send(&mut self, msg: T) -> Result<(), TrySendError<T>> {
        self.do_send_b(msg)
    }

    fn do_send_b(&mut self, msg: T) -> Result<(), TrySendError<T>> {
        self.queue_push_and_signal(msg);

        Ok(())
    }

    fn queue_push_and_signal(&self, msg: T) {
        {
            let mut mc = self.inner.message.borrow_mut();
            let old_v = mc.take();
            *mc = Some(Value::new(old_v, msg));
        }
        self.inner.receivers.borrow().iter().for_each(|r| r.wake());
    }

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), SendError>> {
        if !self.inner.state.borrow().is_open() {
            return Poll::Ready(Err(SendError { kind: SendErrorKind::Disconnected }));
        }
        Poll::Ready(Ok(()))
    }

    fn is_closed(&self) -> bool {
        !self.inner.state.borrow().is_open()
    }

    fn close_channel(&self) {
        self.inner.set_closed();
        self.inner.receivers.borrow_mut().drain(..).for_each(|w| w.wake());
    }
}

impl<T: Clone> Sender<T> {
    pub fn try_send(&mut self, msg: T) -> Result<(), TrySendError<T>> {
        if let Some(inner) = &mut self.0 {
            inner.try_send(msg)
        } else {
            Err(TrySendError { err: SendError { kind: SendErrorKind::Disconnected }, val: msg })
        }
    }

    pub fn start_send(&mut self, msg: T) -> Result<(), SendError> {
        self.try_send(msg).map_err(|e| e.err)
    }

    pub fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), SendError>> {
        let inner = self.0.as_mut().ok_or(SendError { kind: SendErrorKind::Disconnected })?;
        inner.poll_ready(cx)
    }

    pub fn is_closed(&self) -> bool {
        self.0.as_ref().map(SenderInner::is_closed).unwrap_or(true)
    }

    pub fn close_channel(&mut self) {
        if let Some(inner) = &mut self.0 {
            inner.close_channel();
        }
    }

    pub fn disconnect(&mut self) {
        self.0 = None;
    }
}

impl<T: Clone> Clone for Receiver<T> {
    fn clone(&self) -> Receiver<T> {
        let r = Receiver {
            inner: self.inner.clone(),
            waker: Rc::new(AtomicWaker::new()),
            version: self.version,
        };

        if let Some(inner) = self.inner.as_ref() {
            inner.receivers.borrow_mut().push(r.waker.clone());
        }
        r
    }
}

impl<T: Clone> Drop for SenderInner<T> {
    fn drop(&mut self) {
        self.close_channel();
    }
}

impl<T: Clone> Receiver<T> {
    pub fn close(&mut self) {
        if let Some(inner) = &mut self.inner {
            let w = &self.waker;
            let mut recvs = inner.receivers.borrow_mut();
            if let Some(pos) = recvs.iter().position(|r| Rc::ptr_eq(&r, w)) {
                recvs.remove(pos);
            }
        }
    }

    pub fn try_next(&mut self) -> Result<Option<T>, TryRecvError> {
        match self.next_message() {
            Poll::Ready(msg) => Ok(msg),
            Poll::Pending => Err(TryRecvError { _priv: () }),
        }
    }

    fn next_message(&mut self) -> Poll<Option<T>> {
        let inner = self.inner.as_ref().expect("Receiver::next_message called after `None`");

        if let Some(msg) = inner.message.borrow().as_ref() {
            if msg.version != self.version {
                self.version = msg.version;
                return Poll::Ready(Some(msg.value.clone()));
            } else {
                return Poll::Pending;
            }
        } else {
            return Poll::Pending;
        }
    }
}

impl<T: Clone> FusedStream for Receiver<T> {
    fn is_terminated(&self) -> bool {
        self.inner.is_none()
    }
}

impl<T: Clone> Stream for Receiver<T> {
    type Item = T;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<T>> {
        match self.next_message() {
            Poll::Ready(msg) => {
                if msg.is_none() {
                    self.inner = None;
                }
                Poll::Ready(msg)
            }
            Poll::Pending => {
                self.waker.register(cx.waker());
                return Poll::Pending;
            }
        }
    }
}

impl<T: Clone> Drop for Receiver<T> {
    fn drop(&mut self) {
        self.close();
    }
}

impl<T: Clone> Inner<T> {
    // Clear `open` flag in the state, keep `num_messages` intact.
    fn set_closed(&self) {
        if !self.state.borrow().is_open() {
            return;
        }

        self.state.borrow_mut().close();
    }
}

use futures::Sink;
impl<T: Clone> Sink<T> for Sender<T> {
    type Error = SendError;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        (*self).poll_ready(cx)
    }

    fn start_send(mut self: Pin<&mut Self>, msg: T) -> Result<(), Self::Error> {
        (*self).start_send(msg)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match (*self).poll_ready(cx) {
            Poll::Ready(Err(ref e)) if e.is_disconnected() => {
                // If the receiver disconnected, we consider the sink to be flushed.
                Poll::Ready(Ok(()))
            }
            x => x,
        }
    }

    fn poll_close(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.disconnect();
        Poll::Ready(Ok(()))
    }
}
