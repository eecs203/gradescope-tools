use std::time::Duration;

use tokio::sync::{Mutex, MutexGuard};
use tokio::time::sleep;

pub struct RateLimited<T> {
    t: Mutex<T>,
    delay: Duration,
}

impl<T> RateLimited<T> {
    pub fn new(t: T, delay: Duration) -> Self {
        Self {
            t: Mutex::new(t),
            delay,
        }
    }

    pub async fn get(&self) -> MutexGuard<T> {
        let guard = self.t.lock().await;
        sleep(self.delay).await;
        guard
    }
}