use futures::task::LocalSpawnExt;

use futures::executor::LocalPool;
use futures::sink::SinkExt;
use futures::stream::StreamExt;
use rmachine::channel::mpsc;

use rmachine::channel::spsc;
//use rmachine::channel::oneshot;
use futures_timer as timer;
use rmachine::channel::watch;
use tokio;

trait Toto {
    fn plop();
}

trait Mi {}
struct Zut<I: Mi> {
    v: I,
}

fn main() {
    let mut rt = tokio::runtime::Runtime::new().unwrap();
    let ls = tokio::task::LocalSet::new();

    ls.spawn_local(async move {
        //  let (mut tx, mut rx) = watch::channel();
        let (mut tx, mut rx) = mpsc::unbounded();

        let mut tx = tx.clone();
        let mut tx = tx.clone();
        let mut tx = tx.clone();
        //    let mut tx2 = tx.clone();
        let _ = rmachine::spawner::spawn(async move {
            loop {
                println!("send 1");
                tx.unbounded_send("task 1");
                tx.send("Task 1").await.unwrap();
                //_yield().await;
                //   timer::Delay::new(std::time::Duration::from_millis(1)).await;
            }
        });
        /*
         let _ = pool.spawner().spawn_local(async move {
             loop {
                 tx2.send("Task 2").await.unwrap();
             }
         });

         let (mut otx, mut orx) = spsc::channel(20);

         let _ = pool.spawner().spawn_local(async move {
             loop {
               //  println!("send 1");
                 otx.send("ST1").await.unwrap();
               //  println!("send 1 Ok");
             }
         });

         let _ = pool.spawner().spawn_local(async move {
             while let Some(machine) = orx.next().await {
        //         println!("Simple producer {:?}", machine);
             }
         });

         let (ontx, onrx) = oneshot::channel();

         let _ = pool.spawner().spawn_local(async move {
            // println!("send 1");
             ontx.send("GGGGGGGGGGGGG");
          //   println!("send 1 Ok");
         });

         let _ = pool.spawner().spawn_local(async move {
             let res = onrx.await;
             println!("Simple producer {:?}", res);
         });*/

        rmachine::spawner::spawn(async move {
            while let Some(_) = rx.next().await {
                println!("OOOOO");
                //timer::Delay::new(std::time::Duration::from_secs(1)).await;
                //   rx.next().await;
            }
            ()
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
