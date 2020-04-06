#![allow(non_snake_case)]
use futures::future::FutureExt;
use futures::task;
use futures::task::Poll;
use futures::Future;
use std::pin::Pin;

macro_rules! generate {
    ($(
        $(#[$doc:meta])*
        ($Select:ident, $new:ident, $SelectResult:ident, <A, $($B:ident),*>),
    )*) => ($(
        $(#[$doc])*
        #[must_use = "futures do nothing unless polled"]
        pub struct $Select<A, $($B),*>
            where A: Future,
                  $($B: Future),*
        {
            inner: Option<(A, $($B),* )>,
        }

        pub enum $SelectResult<A ,$($B),*> {
            A(A),
            $( $B ($B)),*
        }

        pub fn $new<A, $($B),*>(a: A, $($B: $B),*) -> $Select<A, $($B),*>
            where A: Future,
                  $($B: Future),*
        {
            $Select {
                inner : Some((a,$($B),*))
            }
        }


        impl<A, $($B),*> Future for $Select<A, $($B),*>
            where A: Future + Unpin,
                  $($B: Future + Unpin ),*
        {
            type Output = $SelectResult<A::Output ,$($B::Output),*>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> Poll<Self::Output> {

                let (mut a,$(mut $B),*) = self.inner.take().expect("cannot poll Select twice");

                match a.poll_unpin(cx) {
                    Poll::Ready(res) => { return Poll::Ready($SelectResult::A(res)) },
                    _ => {}
                }

                $(match $B.poll_unpin(cx) {
                    Poll::Ready(res) => { return Poll::Ready($SelectResult::$B(res)) },
                    _ => {}
                })
            *

            self.inner = Some((a,$( $B),*));

                return Poll::Pending;
            }
        }

    )*)
}

generate! {
    /// Future for the `join` combinator, waiting for two futures to
    /// complete.
    ///
    /// This is created by the `Future::join` method.
    (Select, select_new, SelectResult, <A, B>),

    /// Future for the `join3` combinator, waiting for three futures to
    /// complete.
    ///
    /// This is created by the `Future::join3` method.
    (Select3, select_new3, SelectResult3, <A, B, C>),

    /// Future for the `join4` combinator, waiting for four futures to
    /// complete.
    ///
    /// This is created by the `Future::join4` method.
    (Select4, select_new4, SelectResult4, <A, B, C, D>),

    /// Future for the `join5` combinator, waiting for five futures to
    /// complete.
    ///
    /// This is created by the `Future::join5` method.
    (Select5, select_new5, SelectResult5, <A, B, C, D, E>),
}

pub fn select<Fut1, Fut2>(future1: Fut1, future2: Fut2) -> Select<Fut1, Fut2>
where
    Fut1: Future,
    Fut2: Future,
{
    select_new(future1, future2)
}

pub fn select3<Fut1, Fut2, Fut3>(
    future1: Fut1,
    future2: Fut2,
    future3: Fut3,
) -> Select3<Fut1, Fut2, Fut3>
where
    Fut1: Future,
    Fut2: Future,
    Fut3: Future,
{
    select_new3(future1, future2, future3)
}

pub fn select4<Fut1, Fut2, Fut3, Fut4>(
    future1: Fut1,
    future2: Fut2,
    future3: Fut3,
    future4: Fut4,
) -> Select4<Fut1, Fut2, Fut3, Fut4>
where
    Fut1: Future,
    Fut2: Future,
    Fut3: Future,
    Fut4: Future,
{
    select_new4(future1, future2, future3, future4)
}

pub fn select5<Fut1, Fut2, Fut3, Fut4, Fut5>(
    future1: Fut1,
    future2: Fut2,
    future3: Fut3,
    future4: Fut4,
    future5: Fut5,
) -> Select5<Fut1, Fut2, Fut3, Fut4, Fut5>
where
    Fut1: Future,
    Fut2: Future,
    Fut3: Future,
    Fut4: Future,
    Fut5: Future,
{
    select_new5(future1, future2, future3, future4, future5)
}
