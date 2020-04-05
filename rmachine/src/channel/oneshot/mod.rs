use super::atomic_waker::AtomicWaker;
use core::fmt;
use core::pin::Pin;
use futures::future::Future;
use futures::task::{Context, Poll};
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Debug)]
pub struct Receiver<T> {
    waker: Rc<AtomicWaker>,
    inner: Rc<RefCell<Inner<T>>>,
}

#[derive(Debug)]
pub struct Sender<T> {
    inner: Rc<RefCell<Inner<T>>>,
}

impl<T> Unpin for Receiver<T> {}
impl<T> Unpin for Sender<T> {}

#[derive(Debug)]
struct Inner<T> {
    complete: bool,
    data: Option<T>,
    receivers: Vec<Rc<AtomicWaker>>,
}

pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
    let inner = Rc::new(RefCell::new(Inner::new()));
    let receiver = Receiver { inner: inner.clone(), waker: Rc::new(AtomicWaker::new()) };
    let sender = Sender { inner };
    receiver.inner.borrow_mut().receivers.push(receiver.waker.clone());
    (sender, receiver)
}

impl<T> Inner<T> {
    fn new() -> Inner<T> {
        Inner { complete: false, data: None, receivers: vec![] }
    }

    fn send(&mut self, t: T) -> Result<(), T> {
        if self.complete {
            return Err(t);
        }
        assert!(self.data.is_none());
        self.data = Some(t);
        Ok(())
    }

    fn poll_canceled(&self, _: &mut Context<'_>) -> Poll<()> {
        return Poll::Ready(());
    }

    fn is_canceled(&self) -> bool {
        self.complete
    }

    fn drop_tx(&mut self) {
        self.complete = true;

        self.receivers.drain(..).for_each(|r| {
            r.wake();
        });
    }

    fn recv(&mut self) -> Poll<Result<T, Canceled>> {
        let done = if self.complete { true } else { false };

        if done {
            if let Some(data) = self.data.take() {
                return Poll::Ready(Ok(data));
            }
            Poll::Ready(Err(Canceled))
        } else {
            Poll::Pending
        }
    }
}

impl<T> Sender<T> {
    pub fn send(self, t: T) -> Result<(), T> {
        match self.inner.try_borrow_mut() {
            Ok(mut v) => v.send(t),
            Err(_) => Err(t),
        }
    }

    pub fn poll_canceled(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        match self.inner.try_borrow_mut() {
            Ok(v) => v.poll_canceled(cx),
            Err(_) => Poll::Pending,
        }
    }

    pub fn is_canceled(&self) -> bool {
        self.inner.borrow().is_canceled()
    }

    pub fn value(self) -> Option<T> {
        self.inner.borrow_mut().data.take()
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        self.inner.borrow_mut().drop_tx()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Canceled;

impl fmt::Display for Canceled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "oneshot canceled")
    }
}

impl std::error::Error for Canceled {}

impl<T> Receiver<T> {
    pub fn close(&mut self) {
        let mut inner = self.inner.borrow_mut();
        let recvs = &mut inner.receivers;

        let w = &self.waker;
        if let Some(pos) = recvs.iter().position(|r| Rc::ptr_eq(&r, w)) {
            recvs.remove(pos);
        }
    }
}

impl<T> Future for Receiver<T> {
    type Output = Result<T, Canceled>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<T, Canceled>> {
        match self.inner.borrow_mut().recv() {
            Poll::Ready(msg) => Poll::Ready(msg),
            Poll::Pending => {
                self.waker.register(cx.waker());
                return Poll::Pending;
            }
        }
    }
}

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        self.close();
    }
}

impl<T: Clone> Clone for Receiver<T> {
    fn clone(&self) -> Receiver<T> {
        let r = Receiver { inner: self.inner.clone(), waker: Rc::new(AtomicWaker::new()) };
        self.inner.borrow_mut().receivers.push(r.waker.clone());
        r
    }
}
