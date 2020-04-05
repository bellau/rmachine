use async_trait::async_trait;
use env_logger;
use futures::select;
use futures::stream::StreamExt;
use futures::FutureExt;
use log::LevelFilter;
use rmachine::prelude::*;

#[derive(Clone, Debug)]
struct SaveThePlanet;
impl Output for SaveThePlanet {}
impl OutputHandler<SaveThePlanet> for Light {}

#[derive(Debug)]
struct Change;
impl InputHandler<Change> for Light {}

#[derive(Debug)]
struct ButtonPressed;
impl InputHandler<ButtonPressed> for Light {}
#[derive(Debug)]
struct Thunder;
impl InputHandler<Thunder> for Light {}

#[derive(Debug)]
struct EndOfLife;
impl InputHandler<EndOfLife> for Light {}

#[derive(Clone, Debug)]
struct Light {
    light_state: LightState,
    broken: bool,
}
#[derive(Debug, Clone)]
enum LightState {
    On,
    Off,
}

impl Machine for Light {
    type End = ();
}

struct Off;
#[async_trait(?Send)]
impl State for Off {
    type Machine = Light;
    async fn run(&self, context: &StateContext<Off>) -> Never {
        select! {
                  _ = context.accept::<ButtonPressed>() => context.transition(|state| { On }).await,
                  _ = context.accept::<EndOfLife>() => context.transition(|state| { Broken(false) }).await,
        }
    }

    fn machine(self: &Self) -> Self::Machine {
        Light { light_state: LightState::Off, broken: false }
    }
}

struct Broken(bool);
#[async_trait(?Send)]
impl State for Broken {
    type Machine = Light;
    async fn run(&self, c: &StateContext<Self>) -> Never {
        let button_pressed_transition =
            if self.0 { c.transition(|_| On) } else { c.transition(|_| Off) };
        select! {
            _ = c.accept::<ButtonPressed>() =>  c.transition(|s| Broken(!s.0)).await,
            _ = c.accept::<Change>() => {
                println!("change the bubble");
                button_pressed_transition.await
            }
        }
    }

    fn machine(self: &Self) -> Self::Machine {
        Light { light_state: LightState::Off, broken: true }
    }
}

struct On;
#[async_trait(?Send)]
impl State for On {
    type Machine = Light;
    async fn run(&self, c: &StateContext<Self>) -> Never {
        let s = self;
        select! {
            _ = c.accept::<ButtonPressed>() => c.transition(|_| Off).await,
            _ = c.accept::<Thunder>() => { s.chaos(c); c.transition(|_| Off).await},
            _ = c.accept::<EndOfLife>() => c.transition(|_| Broken(true)).await,
            _ = futures_timer::Delay::new(std::time::Duration::from_millis(200)).fuse() => {
                c.output(SaveThePlanet);
            //    c.never().await
            c.transition(|_| Off).await
            }

        }
    }

    fn machine(self: &Self) -> Self::Machine {
        Light { light_state: LightState::On, broken: false }
    }
}

impl On {
    fn chaos(&self, c: &StateContext<Self>) {
        let s = my_stream();
        c.stream_as_input(s);
    }
}

fn my_stream() -> Box<dyn futures::stream::Stream<Item = ButtonPressed> + Unpin> {
    let (tx, rx) = futures::channel::mpsc::unbounded();

    let _ = rmachine::spawner::spawn(async move {
        for _ in 0..10 as u32 {
            let o: u64 = rand::random();
            futures_timer::Delay::new(std::time::Duration::from_millis(o % 1000)).await;
            println!("chaos !!");
            let _ = tx.unbounded_send(ButtonPressed);
        }
    });
    Box::new(rx)
}

fn main() -> () {
    env_logger::builder()
        .default_format()
        .filter(None, LevelFilter::Info)
        .filter(Some("rmachine"), LevelFilter::Debug)
        .init();

    let mut rt = tokio::runtime::Runtime::new().unwrap();

    let ls = tokio::task::LocalSet::new();

    ls.spawn_local(async move {
        let light = machine(Off);

        let mut m_rx = light.machine();
        rmachine::spawner::spawn(async move {
            while let Some(machine) = m_rx.next().await {
                println!("State : {:?}", machine);
            }
        });

        rmachine::spawner::spawn(async move {
            for i in 0..100 as usize {
                println!("press button");
                light.input(ButtonPressed);

                futures_timer::Delay::new(std::time::Duration::from_millis(
                    rand::random::<u64>() % 20,
                ))
                .await;

                if i == 20 {
                    light.input(Change);
                }
            }

            light.input(ButtonPressed);
            light.input(Thunder);
            _yield().await;

            light.input(ButtonPressed);
            _yield().await;
            light.input(Thunder);

            _yield().await;
            light.input(ButtonPressed);
            _yield().await;
            light.input(ButtonPressed);
            _yield().await;

            light.input(ButtonPressed);
            _yield().await;

            _yield().await;

            light.input(ButtonPressed);
            _yield().await;

            light.input(Thunder);
            _yield().await;

            light.input(ButtonPressed);
            _yield().await;
        });
    });

    rt.block_on(ls);
}

pub async fn _yield() {
    use futures::*;
    use std::pin::Pin;
    use std::task::*;
    /// Yield implementation
    struct YieldNow {
        yielded: bool,
    }

    impl Future for YieldNow {
        type Output = ();

        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
            if self.yielded {
                return Poll::Ready(());
            }

            self.yielded = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }

    YieldNow { yielded: false }.await
}
