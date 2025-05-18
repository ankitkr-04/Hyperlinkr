use std::time::{Instant, Duration};

use tokio::sync::Mutex;



pub struct CircuitBreaker {
  failures : Mutex<u32>,
  last_failure : Mutex<Instant>,
}


impl CircuitBreaker {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn record_failure(&self) {
        let mut failures = self.failures.lock().await;
        *failures += 1;
        let mut last_failure = self.last_failure.lock().await;
        *last_failure = Instant::now();
    }

    pub async fn should_try(&self) -> bool {
        let failures = *self.failures.lock().await;
        let last = *self.last_failure.lock().await;
        failures < 10 || last.elapsed() > Duration::from_secs(1)
    }
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self {
            failures: Mutex::new(0),
            last_failure: Mutex::new(Instant::now()),
        }
    }
    
}