use super::atomic_waker::AtomicWaker;
use core::fmt;
use core::pin::Pin;
use futures::stream::*;
use futures::task::{Context, Poll, Waker};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::*;
mod sink_impl;

#[derive(Debug)]
struct SenderInner<T> {
    inner: Rc<Inner<T>>,
    maybe_parked: bool,
}

impl<T> Unpin for SenderInner<T> {}

#[derive(Debug)]
pub struct Sender<T>(Option<SenderInner<T>>);

#[derive(Debug)]
pub struct UnboundedSender<T>(Option<SenderInner<T>>);

#[derive(Debug)]
pub struct Receiver<T> {
    inner: Option<Rc<Inner<T>>>,
}

#[derive(Debug)]
pub struct UnboundedReceiver<T>(Receiver<T>);

impl<T> Unpin for UnboundedReceiver<T> {}

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
    Full,
    Disconnected,
}

pub struct TryRecvError {
    _priv: (),
}

impl fmt::Display for SendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_full() {
            write!(f, "send failed because channel is full")
        } else {
            write!(f, "send failed because receiver is gone")
        }
    }
}

impl std::error::Error for SendError {}

impl SendError {
    /// Returns true if this error is a result of the channel being full.
    pub fn is_full(&self) -> bool {
        match self.kind {
            SendErrorKind::Full => true,
            _ => false,
        }
    }

    /// Returns true if this error is a result of the receiver being dropped.
    pub fn is_disconnected(&self) -> bool {
        match self.kind {
            SendErrorKind::Disconnected => true,
            _ => false,
        }
    }
}

impl<T> fmt::Debug for TrySendError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TrySendError").field("kind", &self.err.kind).finish()
    }
}

impl<T> fmt::Display for TrySendError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_full() {
            write!(f, "send failed because channel is full")
        } else {
            write!(f, "send failed because receiver is gone")
        }
    }
}

impl<T: core::any::Any> std::error::Error for TrySendError<T> {}

impl<T> TrySendError<T> {
    /// Returns true if this error is a result of the channel being full.
    pub fn is_full(&self) -> bool {
        self.err.is_full()
    }

    /// Returns true if this error is a result of the receiver being dropped.
    pub fn is_disconnected(&self) -> bool {
        self.err.is_disconnected()
    }

    /// Returns the message that was attempted to be sent but failed.
    pub fn into_inner(self) -> T {
        self.val
    }

    /// Drops the message and converts into a `SendError`.
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

#[derive(Debug)]
struct Inner<T> {
    buffer: Option<usize>,

    state: RefCell<InnerState>,
    message_queue: RefCell<VecDeque<T>>,

    sender_task: RefCell<SenderTask>,

    num_senders: AtomicUsize,

    recv_task: AtomicWaker,
}

#[derive(Debug)]
enum InnerState {
    Init(usize),
    Closed,
}

impl InnerState {
    fn is_open(&self) -> bool {
        if let InnerState::Init(_) = self {
            true
        } else {
            false
        }
    }

    fn num_messages(&self) -> usize {
        match self {
            InnerState::Init(m) => *m,
            _ => 0,
        }
    }
    fn inc_num_messages(&mut self) -> Option<usize> {
        match self {
            InnerState::Init(m) => {
                *m += 1;
                Some(*m)
            }
            _ => None,
        }
    }

    fn dec_num_messages(&mut self) {
        match self {
            InnerState::Init(m) => {
                *m += 1;
            }
            _ => {}
        }
    }

    fn close(&mut self) {
        *self = Self::Closed;
    }
}
// The `is_open` flag is stored in the left-most bit of `Inner::state`
const OPEN_MASK: usize = usize::max_value() - (usize::max_value() >> 1);

// The maximum number of messages that a channel can track is `usize::max_value() >> 1`
const MAX_CAPACITY: usize = !(OPEN_MASK);

// The maximum requested buffer size must be less than the maximum capacity of
// a channel. This is because each sender gets a guaranteed slot.
const MAX_BUFFER: usize = MAX_CAPACITY >> 1;

// Sent to the consumer to wake up blocked producers
#[derive(Debug)]
struct SenderTask {
    task: Option<Waker>,
    is_parked: bool,
}

impl SenderTask {
    fn new() -> Self {
        SenderTask { task: None, is_parked: false }
    }

    fn notify(&mut self) {
        self.is_parked = false;

        if let Some(task) = self.task.take() {
            task.wake();
        }
    }
}

pub fn channel<T>(buffer: usize) -> (Sender<T>, Receiver<T>) {
    // Check that the requested buffer size does not exceed the maximum buffer
    // size permitted by the system.
    assert!(buffer < MAX_BUFFER, "requested buffer size too large");
    let (tx, rx) = channel2(Some(buffer));
    (Sender(Some(tx)), rx)
}

pub fn unbounded<T>() -> (UnboundedSender<T>, UnboundedReceiver<T>) {
    let (tx, rx) = channel2(None);
    (UnboundedSender(Some(tx)), UnboundedReceiver(rx))
}

fn channel2<T>(buffer: Option<usize>) -> (SenderInner<T>, Receiver<T>) {
    let inner = Rc::new(Inner {
        buffer,
        state: RefCell::new(InnerState::Init(0)),
        message_queue: RefCell::new(VecDeque::new()),
        num_senders: AtomicUsize::new(1),
        recv_task: AtomicWaker::new(),
        sender_task: RefCell::new(SenderTask::new()),
    });

    let tx = SenderInner { inner: inner.clone(), maybe_parked: false };

    let rx = Receiver { inner: Some(inner) };

    (tx, rx)
}
impl<T> SenderInner<T> {
    fn try_send(&mut self, msg: T) -> Result<(), TrySendError<T>> {
        if !self.poll_unparked(None).is_ready() {
            return Err(TrySendError { err: SendError { kind: SendErrorKind::Full }, val: msg });
        }

        self.do_send_b(msg)
    }

    fn do_send_b(&mut self, msg: T) -> Result<(), TrySendError<T>> {
        let park_self = match self.inc_num_messages() {
            Some(num_messages) => num_messages > self.inner.buffer.unwrap(),
            None => {
                return Err(TrySendError {
                    err: SendError { kind: SendErrorKind::Disconnected },
                    val: msg,
                })
            }
        };
        if park_self {
            self.park();
        }

        self.queue_push_and_signal(msg);

        Ok(())
    }

    fn poll_ready_nb(&self) -> Poll<Result<(), SendError>> {
        let state = self.inner.state.borrow();

        if state.is_open() {
            Poll::Ready(Ok(()))
        } else {
            Poll::Ready(Err(SendError { kind: SendErrorKind::Disconnected }))
        }
    }
    fn queue_push_and_signal(&self, msg: T) {
        self.inner.message_queue.borrow_mut().push_back(msg);
        self.inner.recv_task.wake();
    }

    fn inc_num_messages(&self) -> Option<usize> {
        self.inner.state.borrow_mut().inc_num_messages()
    }

    fn park(&mut self) {
        let mut sender = self.inner.sender_task.borrow_mut();
        sender.task = None;
        sender.is_parked = true;
        self.maybe_parked = true;
    }

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), SendError>> {
        if !self.inner.state.borrow().is_open() {
            return Poll::Ready(Err(SendError { kind: SendErrorKind::Disconnected }));
        }

        self.poll_unparked(Some(cx)).map(Ok)
    }

    fn same_receiver(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }

    fn is_closed(&self) -> bool {
        !self.inner.state.borrow().is_open()
    }

    fn close_channel(&self) {
        self.inner.set_closed();
        self.inner.recv_task.wake();
    }

    fn poll_unparked(&mut self, cx: Option<&mut Context<'_>>) -> Poll<()> {
        if self.maybe_parked {
            let mut task = self.inner.sender_task.borrow_mut();

            if !task.is_parked {
                self.maybe_parked = false;
                return Poll::Ready(());
            }
            task.task = cx.map(|cx| cx.waker().clone());

            Poll::Pending
        } else {
            Poll::Ready(())
        }
    }
}

impl<T> Sender<T> {
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

    pub fn same_receiver(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (Some(inner), Some(other)) => inner.same_receiver(other),
            _ => false,
        }
    }
}

impl<T> UnboundedSender<T> {
    pub fn poll_ready(&self, _: &mut Context<'_>) -> Poll<Result<(), SendError>> {
        let inner = self.0.as_ref().ok_or(SendError { kind: SendErrorKind::Disconnected })?;
        inner.poll_ready_nb()
    }

    /// Returns whether this channel is closed without needing a context.
    pub fn is_closed(&self) -> bool {
        self.0.as_ref().map(SenderInner::is_closed).unwrap_or(true)
    }

    /// Closes this channel from the sender side, preventing any new messages.
    pub fn close_channel(&self) {
        if let Some(inner) = &self.0 {
            inner.close_channel();
        }
    }

    /// Disconnects this sender from the channel, closing it if there are no more senders left.
    pub fn disconnect(&mut self) {
        self.0 = None;
    }

    // Do the send without parking current task.
    fn do_send_nb(&self, msg: T) -> Result<(), TrySendError<T>> {
        if let Some(inner) = &self.0 {
            if inner.inc_num_messages().is_some() {
                inner.queue_push_and_signal(msg);
                return Ok(());
            }
        }

        Err(TrySendError { err: SendError { kind: SendErrorKind::Disconnected }, val: msg })
    }

    /// Send a message on the channel.
    ///
    /// This method should only be called after `poll_ready` has been used to
    /// verify that the channel is ready to receive a message.
    pub fn start_send(&mut self, msg: T) -> Result<(), SendError> {
        self.do_send_nb(msg).map_err(|e| e.err)
    }

    /// Sends a message along this channel.
    ///
    /// This is an unbounded sender, so this function differs from `Sink::send`
    /// by ensuring the return type reflects that the channel is always ready to
    /// receive messages.
    pub fn unbounded_send(&self, msg: T) -> Result<(), TrySendError<T>> {
        self.do_send_nb(msg)
    }

    /// Returns whether the senders send to the same receiver.
    pub fn same_receiver(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (Some(inner), Some(other)) => inner.same_receiver(other),
            _ => false,
        }
    }
}

impl<T> Drop for SenderInner<T> {
    fn drop(&mut self) {
        // Ordering between variables don't matter here
        let prev = self.inner.num_senders.fetch_sub(1, SeqCst);

        if prev == 1 {
            self.close_channel();
        }
    }
}

/*
 *
 * ===== impl Receiver =====
 *
 */

impl<T> Receiver<T> {
    /// Closes the receiving half of a channel, without dropping it.
    ///
    /// This prevents any further messages from being sent on the channel while
    /// still enabling the receiver to drain messages that are buffered.
    pub fn close(&mut self) {
        if let Some(inner) = &mut self.inner {
            inner.set_closed();
            inner.sender_task.borrow_mut().notify();
        }
    }

    /// Tries to receive the next message without notifying a context if empty.
    ///
    /// It is not recommended to call this function from inside of a future,
    /// only when you've otherwise arranged to be notified when the channel is
    /// no longer empty.
    ///
    /// This function will panic if called after `try_next` or `poll_next` has
    /// returned None.
    pub fn try_next(&mut self) -> Result<Option<T>, TryRecvError> {
        match self.next_message() {
            Poll::Ready(msg) => Ok(msg),
            Poll::Pending => Err(TryRecvError { _priv: () }),
        }
    }

    fn next_message(&mut self) -> Poll<Option<T>> {
        let pop = {
            let inner = &mut self.inner;

            let inner = inner.as_mut().expect("Receiver::next_message called after `None`");
            inner.message_queue.borrow_mut().pop_front()
        };

        match pop {
            Some(msg) => {
                self.unpark_one();
                self.dec_num_messages();

                Poll::Ready(Some(msg))
            }
            None => {
                {
                    let state = self.inner.as_ref().expect("blalba").state.borrow();
                    if state.is_open() || state.num_messages() != 0 {
                        return Poll::Pending;
                    }
                }
                self.inner = None;
                Poll::Ready(None)
            }
        }
    }

    // Unpark a single task handle if there is one pending in the parked queue
    fn unpark_one(&mut self) {
        if let Some(inner) = &mut self.inner {
            inner.sender_task.borrow_mut().notify();
        }
    }

    fn dec_num_messages(&self) {
        if let Some(inner) = &self.inner {
            // OPEN_MASK is highest bit, so it's unaffected by subtraction
            // unless there's underflow, and we know there's no underflow
            // because number of messages at this point is always > 0.
            inner.state.borrow_mut().dec_num_messages();
        }
    }
}

// The receiver does not ever take a Pin to the inner T
impl<T> Unpin for Receiver<T> {}

impl<T> FusedStream for Receiver<T> {
    fn is_terminated(&self) -> bool {
        self.inner.is_none()
    }
}

impl<T> Stream for Receiver<T> {
    type Item = T;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<T>> {
        // Try to read a message off of the message queue.
        match self.next_message() {
            Poll::Ready(msg) => {
                if msg.is_none() {
                    self.inner = None;
                }
                Poll::Ready(msg)
            }
            Poll::Pending => {
                // There are no messages to read, in this case, park.
                self.inner.as_ref().unwrap().recv_task.register(cx.waker());
                // Check queue again after parking to prevent race condition:
                // a message could be added to the queue after previous `next_message`
                // before `register` call.
                self.next_message()
            }
        }
    }
}

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        // Drain the channel of all pending messages
        self.close();
        if self.inner.is_some() {
            while let Poll::Ready(Some(..)) = self.next_message() {
                // ...
            }
        }
    }
}

impl<T> UnboundedReceiver<T> {
    /// Closes the receiving half of the channel, without dropping it.
    ///
    /// This prevents any further messages from being sent on the channel while
    /// still enabling the receiver to drain messages that are buffered.
    pub fn close(&mut self) {
        self.0.close();
    }

    /// Tries to receive the next message without notifying a context if empty.
    ///
    /// It is not recommended to call this function from inside of a future,
    /// only when you've otherwise arranged to be notified when the channel is
    /// no longer empty.
    ///
    /// This function will panic if called after `try_next` or `poll_next` has
    /// returned None.
    pub fn try_next(&mut self) -> Result<Option<T>, TryRecvError> {
        self.0.try_next()
    }
}

impl<T> FusedStream for UnboundedReceiver<T> {
    fn is_terminated(&self) -> bool {
        self.0.is_terminated()
    }
}

impl<T> Stream for UnboundedReceiver<T> {
    type Item = T;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<T>> {
        Pin::new(&mut self.0).poll_next(cx)
    }
}

/*
 *
 * ===== impl Inner =====
 *
 */

impl<T> Inner<T> {
    // Clear `open` flag in the state, keep `num_messages` intact.
    fn set_closed(&self) {
        if !self.state.borrow().is_open() {
            return;
        }

        self.state.borrow_mut().close();
    }
}
