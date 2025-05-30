use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{info, warn};
use rand::seq::SliceRandom;
use rand::rng;

#[derive(Clone)]
struct NodeState {
    failure_count: u32,
    last_failure: Instant,
    is_healthy: bool,
}

pub struct CircuitBreaker {
    state: RwLock<HashMap<String, NodeState>>,
    nodes: Vec<String>,
    retry_interval: Duration,
    max_failures: u32,
}

impl CircuitBreaker {
    pub fn new(nodes: Vec<String>, max_failures: u32, retry_interval: Duration) -> Self {
        let state = nodes.iter().map(|node| {
            (node.clone(), NodeState {
                failure_count: 0,
                last_failure: Instant::now() - retry_interval,
                is_healthy: true,
            })
        }).collect();
        Self {
            state: RwLock::new(state),
            nodes,
            retry_interval,
            max_failures,
        }
    }

    pub async fn get_healthy_node(&self) -> Option<String> {
        let state = self.state.read().await;
        let mut healthy_nodes: Vec<_> = state.iter()
            .filter(|(_, s)| s.is_healthy || s.last_failure.elapsed() > self.retry_interval)
            .map(|(node, _)| node)
            .collect();
        if healthy_nodes.is_empty() {
            warn!("No healthy nodes available");
            return None;
        }
        healthy_nodes.shuffle(&mut rng());
        healthy_nodes.first().map(|&node| node.clone())
    }

    pub async fn record_failure(&self, node: &str) {
        let mut state = self.state.write().await;
        if let Some(node_state) = state.get_mut(node) {
            node_state.failure_count += 1;
            node_state.last_failure = Instant::now();
            if node_state.failure_count >= self.max_failures {
                node_state.is_healthy = false;
                info!("Circuit breaker tripped for node {}", node);
            }
        }
    }

    pub async fn add_node(&self, node: String) {
        let mut state = self.state.write().await;
        state.entry(node.clone()).or_insert(NodeState {
            failure_count: 0,
            last_failure: Instant::now() - self.retry_interval,
            is_healthy: true,
        });
        info!("Added node {}", node);
    }

    pub async fn reset_unhealthy(&self) {
        let mut state = self.state.write().await;
        for (node, node_state) in state.iter_mut() {
            if !node_state.is_healthy && node_state.last_failure.elapsed() > self.retry_interval {
                node_state.is_healthy = true;
                node_state.failure_count = 0;
                info!("Reset node {}", node);
            }
        }
    }

    pub fn get_node_index(&self, node: &str) -> Option<usize> {
        self.nodes.iter().position(|n| n == node)
    }
}
