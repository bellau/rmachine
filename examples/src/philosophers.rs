use async_trait::async_trait;
use env_logger;
use futures::FutureExt;
use log::LevelFilter;
use rmachine::prelude::*;

#[derive(Clone)]
enum ForkMachine {
    Available,
    Used,
}
impl Machine for ForkMachine {
    type End = ();
}

struct Fork(String);
impl InputHandler<Fork> for ForkMachine {}

struct TakeFork {
    fork: rmachine::channel::oneshot::Sender<Fork>,
}
impl InputHandler<TakeFork> for ForkMachine {}

struct AvailableFork {
    fork: Fork,
}

#[async_trait(?Send)]
impl State for AvailableFork {
    type Machine = ForkMachine;
    async fn run(&self, context: &StateContext<Self>) -> Never
    where
        Self: Sized,
    {
        let take = context.accept::<TakeFork>().await;
        context
            .transition(|s| {
                let ret = UsedFork { name: s.fork.0.clone() };
                let res = take.fork.send(s.fork);
                if let Err(_) = res {
                    panic!("BOMMB");
                }
                ret
            })
            .await
    }

    fn machine(self: &Self) -> Self::Machine {
        ForkMachine::Available
    }
}

struct UsedFork {
    name: String,
}

#[async_trait(?Send)]
impl State for UsedFork {
    type Machine = ForkMachine;
    async fn run(&self, context: &StateContext<Self>) -> Never
    where
        Self: Sized,
    {
        let fork = context.accept::<Fork>().await;
        context.transition(|_| AvailableFork { fork }).await
    }

    fn machine(self: &Self) -> Self::Machine {
        ForkMachine::Used
    }
}

#[derive(Clone)]
enum PhilosopherMachine {
    Thinking,
    Starving,
    Eating,
}

impl Machine for PhilosopherMachine {
    type End = ();
}

struct PhilosopherResource {
    left: LocalStateMachine<ForkMachine>,
    right: LocalStateMachine<ForkMachine>,
    name: String,
}

struct ThinkingPhilosopher(PhilosopherResource);
#[async_trait(?Send)]
impl State for ThinkingPhilosopher {
    type Machine = PhilosopherMachine;

    async fn run(&self, context: &StateContext<Self>) -> Never {
        //      println!("thinking {}",self.0.name);

        futures_timer::Delay::new(std::time::Duration::from_millis(rand::random::<u64>() % 1000))
            .await;
        context.transition(|s| StarvingPhilosopher(s.0)).await
    }
    fn machine(self: &Self) -> Self::Machine {
        PhilosopherMachine::Thinking
    }
}

struct EatingPhilosopher(PhilosopherResource, (Fork, Fork));
#[async_trait(?Send)]
impl State for EatingPhilosopher {
    type Machine = PhilosopherMachine;

    async fn run(&self, context: &StateContext<Self>) -> Never {
        println!("eating {}", self.0.name);
        futures_timer::Delay::new(std::time::Duration::from_millis(10)).await;
        context
            .transition(|s| {
                s.0.left.input((s.1).0);
                s.0.right.input((s.1).1);
                ThinkingPhilosopher(s.0)
            })
            .await
    }
    fn machine(self: &Self) -> Self::Machine {
        PhilosopherMachine::Eating
    }
}

struct StarvingPhilosopher(PhilosopherResource);
#[async_trait(?Send)]
impl State for StarvingPhilosopher {
    type Machine = PhilosopherMachine;

    async fn run(&self, context: &StateContext<Self>) -> Never {
        let (ltx, lrx) = rmachine::channel::oneshot::channel();
        let (rtx, rrx) = rmachine::channel::oneshot::channel();

        self.0.left.input(TakeFork { fork: ltx });
        self.0.right.input(TakeFork { fork: rtx });

        let lrx = lrx.map(Result::unwrap);

        let rrx = rrx.map(Result::unwrap);
        let res = futures::future::join(lrx, rrx).await;
        context.transition(|s| EatingPhilosopher(s.0, res)).await
    }
    fn machine(self: &Self) -> Self::Machine {
        PhilosopherMachine::Starving
    }
}

fn fork(name: &str) -> LocalStateMachine<ForkMachine> {
    rmachine::machine(AvailableFork { fork: Fork(name.into()) })
}

fn philosopher(
    name: &str,
    left: LocalStateMachine<ForkMachine>,
    right: LocalStateMachine<ForkMachine>,
) -> LocalStateMachine<PhilosopherMachine> {
    rmachine::machine(ThinkingPhilosopher(PhilosopherResource { left, right, name: name.into() }))
}

fn main() -> () {
    env_logger::builder()
        .default_format()
        .filter(None, LevelFilter::Info)
        .filter(Some("rmachine"), LevelFilter::Info)
        .init();

    let mut rt = tokio::runtime::Runtime::new().unwrap();

    let ls = tokio::task::LocalSet::new();

    ls.spawn_local(async move {
        let mut forks = vec![];
        for i in 0..1_000 {
            forks.push(fork(&format!("for{}", i)));
        }

        for i in 0..1_000 {
            let left = forks.get(i % 1_000).unwrap();
            let right = forks.get((i + 1) % 1_000).unwrap();
            philosopher(&format!("p{}", i), left.clone(), right.clone());
        }
    });

    rt.block_on(ls);
}
