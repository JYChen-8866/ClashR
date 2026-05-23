use std::future::Future;

use once_cell::sync::OnceCell;
use tokio::runtime::Runtime;
use tokio::sync::oneshot;

static RUNTIME: OnceCell<Runtime> = OnceCell::new();

fn runtime() -> &'static Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("clashr-tokio")
            .build()
            .expect("failed to build tokio runtime")
    })
}

/// Spawn the future on the global tokio runtime and return a future that
/// resolves on the caller's executor (gpui's) when the work is done.
///
/// This bridges gpui's executor with reqwest/hyper/etc which require a
/// running tokio reactor.
pub fn spawn_on_tokio<F, T>(future: F) -> impl Future<Output = T>
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    let (tx, rx) = oneshot::channel();
    runtime().spawn(async move {
        let result = future.await;
        let _ = tx.send(result);
    });
    async move {
        rx.await
            .expect("clashr tokio task was cancelled or panicked")
    }
}

/// Fire-and-forget: spawn a long-running future on the global tokio
/// runtime without waiting for its result. Use this for tasks that run
/// for the lifetime of the app (e.g. WebSocket readers) where you want
/// to avoid creating a second tokio runtime.
pub fn spawn_tokio_task<F>(future: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    runtime().spawn(future);
}
