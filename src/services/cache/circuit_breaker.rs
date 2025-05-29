use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{info, warn};
use rand::seq::SliceRandom;
use rand::thread_rng;

#[derive(Clone)]
struct NodeState {
    failure_count: u32,
    last_failure: Instant,
    is_healthy: bool,
}

pub struct CircuitBreaker {
    state: RwLock<HashMap<String, NodeState>>,
    retry_interval: Duration,
    max_failures: u32,
}

/// Implements a circuit breaker pattern for managing the health of a set of nodes.
/// 
/// # Methods
/// 
/// - `new(nodes, max_failures, retry_interval)`: Constructs a new `CircuitBreaker` with the given nodes, maximum allowed failures before tripping, and the retry interval for unhealthy nodes.
/// - `get_healthy_node(&self) -> Option<String>`: Asynchronously returns a randomly selected healthy node, or a node eligible for retry if its retry interval has elapsed. Returns `None` if no nodes are available.
/// - `record_failure(&self, node: &str)`: Asynchronously records a failure for the specified node, updating its failure count and marking it as unhealthy if the maximum failures threshold is reached.
/// - `add_node(&self, node: String)`: Asynchronously adds a new node to the circuit breaker, initializing its state as healthy.
/// - `reset_unhealthy(&self)`: Asynchronously resets the state of unhealthy nodes whose retry interval has elapsed, marking them as healthy and resetting their failure count.
/// 
/// # Example
/// ```
/// let cb = CircuitBreaker::new(vec!["node1".into(), "node2".into()], 3, Duration::from_secs(30));
/// // Use cb.get_healthy_node().await, cb.record_failure("node1").await, etc.
/// ```
impl CircuitBreaker {
    pub fn new(nodes: Vec<String>, max_failures: u32, retry_interval: Duration) -> Self {
        let state = nodes.into_iter().map(|node| {
            (node, NodeState {
                failure_count: 0,
                last_failure: Instant::now() - retry_interval,
                is_healthy: true,
            })
        }).collect();
        Self {
            state: RwLock::new(state),
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
        healthy_nodes.shuffle(&mut thread_rng());
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
}