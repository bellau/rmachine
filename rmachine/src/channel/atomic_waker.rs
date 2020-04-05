use std::cell::RefCell;
use std::fmt;
use std::task::Waker;

pub struct AtomicWaker {
    inner: RefCell<Inner>,
}

struct Inner {
    state: State,
    waker: Option<Waker>,
}
#[derive(Clone)]
enum State {
    Waiting,
    Waking,
}

impl AtomicWaker {
    /// Create an `AtomicWaker`.
    pub fn new() -> AtomicWaker {
        AtomicWaker { inner: RefCell::new(Inner { state: State::Waiting, waker: None }) }
    }

    pub fn register(&self, waker: &Waker) {
        let mut inner = self.inner.borrow_mut();
        match inner.state {
            State::Waiting => {
                inner.waker = Some(waker.clone());
            }
            State::Waking => {
                println!("wake");

                waker.wake_by_ref();
            }
        }
    }

    pub fn wake(&self) {
        if let Some(waker) = self.take() {
            waker.wake();
        }

        self.inner.borrow_mut().state = State::Waiting;
    }

    pub fn take(&self) -> Option<Waker> {
        let mut inner = self.inner.borrow_mut();
        match inner.state {
            State::Waiting => {
                let waker = inner.waker.take();
                inner.state = State::Waking;

                waker
            }
            _ => None,
        }
    }
}

impl Default for AtomicWaker {
    fn default() -> Self {
        AtomicWaker::new()
    }
}

impl fmt::Debug for AtomicWaker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AtomicWaker")
    }
}
