use std::future::Future;
/*use futures::executor::LocalPool;

thread_local! {
    static CONTEXT: Rc<RefCell<LocalPool>> = Rc::new(RefCell::new(LocalPool::new()));
}

fn current() -> Rc<RefCell<LocalPool>> {
    CONTEXT.with(|ctx| ctx.clone())
}

pub fn wait() {
    current().borrow_mut().run();
}

pub fn spawn<Fut>(future: Fut)
where
    Fut: Future<Output = ()> + 'static,
{
    let _ = current().borrow().spawner().spawn_local(future);
}
*/

use tokio;

/*
thread_local! {

    static CONTEXT: Rc<RefCell<task::LocalSet>> = Rc::new(RefCell::new(task::LocalSet::new()));
}

fn current() -> Rc<RefCell<task::LocalSet>> {
    CONTEXT.with(|ctx| ctx.clone())
}*/

pub fn wait() {}

pub fn spawn<Fut>(future: Fut)
where
    Fut: Future<Output = ()> + 'static,
{
    tokio::task::spawn_local(future);
    //let _ = current().borrow().spawn_local(future);
}
