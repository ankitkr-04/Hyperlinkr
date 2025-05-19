// src/services/cache/circuit_breaker.rs
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

pub struct CircuitBreaker {
    failures: Mutex<u32>,
    last_failure: Mutex<Instant>,
    healthy_nodes: Mutex<Vec<String>>,
}

impl CircuitBreaker {
    pub fn new(nodes: Vec<String>) -> Self {
        Self {
            failures: Mutex::new(0),
            last_failure: Mutex::new(Instant::now()),
            healthy_nodes: Mutex::new(nodes),
        }
    }

    pub async fn should_try(&self, node: &str) -> bool {
        let failures = *self.failures.lock().await;
        let last = *self.last_failure.lock().await;
        let healthy = self.healthy_nodes.lock().await.contains(&node.to_string());
        healthy && (failures < 10 || last.elapsed() > Duration::from_secs(1))
    }

    pub async fn record_failure(&self, node: &str) {
        let mut failures = self.failures.lock().await;
        *failures += 1;
        *self.last_failure.lock().await = Instant::now();
        let mut nodes = self.healthy_nodes.lock().await;
        nodes.retain(|n| n != node);
    }

    pub async fn add_node(&self, node: String) {
        let mut nodes = self.healthy_nodes.lock().await;
        if !nodes.contains(&node) {
            nodes.push(node);
        }
    }
}
