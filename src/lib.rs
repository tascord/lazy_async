use tokio::spawn;

use {
    futures::{
        future::{self, always_ready},
        FutureExt,
    },
    std::{
        future::{Future, IntoFuture},
        pin::Pin,
        sync::{Arc, RwLock},
    },
    tokio::sync::broadcast::{channel, Sender},
};

#[derive(Default, Clone)]
enum LazyAsyncState<T> {
    #[default]
    Waiting,
    Pollable(Sender<T>),
    Completed(T),
}

pub struct LazyAsync<T, F>(Arc<RwLock<LazyAsyncState<T>>>, Arc<RwLock<Option<F>>>)
where
    F: Future<Output = T>;

impl<T, F> Clone for LazyAsync<T, F>
where
    F: Future<Output = T>,
    T: Clone,
{
    fn clone(&self) -> Self {
        Self(self.0.clone(), self.1.clone())
    }
}

impl<T, F> LazyAsync<T, F>
where
    F: Future<Output = T> + Send + Sync + 'static,
    T: Clone + Send + Sync + 'static,
{
    pub fn new(f: F) -> Self {
        Self(Default::default(), Arc::new(RwLock::new(Some(f))))
    }

    fn start(&self) {
        let (tx, _) = channel::<T>(1);
        *self.0.write().unwrap() = LazyAsyncState::Pollable(tx.clone());
        spawn({
            let lock = self.0.clone();
            let f = self.1.write().unwrap().take().unwrap();

            async move {
                let v = f.into_future().await;
                let _ = tx.send(v.clone());
                *lock.write().unwrap() = LazyAsyncState::Completed(v);
            }
        });
    }
}

impl<T, F> IntoFuture for LazyAsync<T, F>
where
    F: Future<Output = T> + Send + Sync + 'static,
    T: Clone + Send + Sync + 'static,
{
    type Output = T;
    type IntoFuture = Pin<Box<dyn future::Future<Output = T>>>;

    fn into_future(self) -> Self::IntoFuture {
        let state = self.0.read().unwrap().clone();
        match state {
            LazyAsyncState::Waiting => {
                self.start();
                self.clone().into_future()
            }

            LazyAsyncState::Pollable(tx) => {
                let mut rx = tx.subscribe();
                async move { rx.recv().await.unwrap() }.boxed_local()
            }

            LazyAsyncState::Completed(v) => always_ready(move || v.clone()).boxed_local(),
        }
    }
}
