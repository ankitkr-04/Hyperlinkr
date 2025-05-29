use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tracing::info;

pub struct CircuitBreaker {
    failures: Mutex<Vec<(String, u32)>>,
    last_failure: Mutex<Vec<(String, Instant)>>,
    healthy_nodes: Mutex<Vec<String>>,
    retry_interval: Duration,
}

impl CircuitBreaker {
    pub fn new(nodes: Vec<String>) -> Self {
        let failures = nodes.iter().map(|n| (n.clone(), 0)).collect();
        let last_failure = nodes.iter().map(|n| (n.clone(), Instant::now())).collect();
        Self {
            failures: Mutex::new(failures),
            last_failure: Mutex::new(last_failure),
            healthy_nodes: Mutex::new(nodes),
            retry_interval: Duration::from_secs(30),
        }
    }

    pub async fn should_try(&self, node: &str) -> bool {
        let failures = self.failures.lock().await;
        let last = self.last_failure.lock().await;
        let healthy = self.healthy_nodes.lock().await.contains(&node.to_string());
        let failure_count = failures.iter().find(|(n, _)| n == node).map(|(_, c)| *c).unwrap_or(0);
        let last_failure = last.iter().find(|(n, _)| n == node).map(|(_, t)| *t).unwrap_or(Instant::now());
        healthy && (failure_count < 10 || last_failure.elapsed() > self.retry_interval)
    }

    pub async fn record_failure(&self, node: &str) {
        let mut failures = self.failures.lock().await;
        if let Some((_, count)) = failures.iter_mut().find(|(n, _)| n == node) {
            *count += 1;
        }
        let mut last = self.last_failure.lock().await;
        if let Some((_, time)) = last.iter_mut().find(|(n, _)| n == node) {
            *time = Instant::now();
        }
        let mut nodes = self.healthy_nodes.lock().await;
        nodes.retain(|n| n != node);
        info!("Circuit breaker tripped for node {}", node);
    }

    pub async fn add_node(&self, node: String) {
        let mut nodes = self.healthy_nodes.lock().await;
        if !nodes.contains(&node) {
            nodes.push(node.clone());
            let mut failures = self.failures.lock().await;
            failures.push((node.clone(), 0));
            let mut last = self.last_failure.lock().await;
            last.push((node, Instant::now()));
        }
    }
}