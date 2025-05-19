// src/services/storage/dragonfly.rs
use bb8::Pool;
use bb8_redis::RedisConnectionManager;
use redis::AsyncCommands;
use std::time::Duration;

pub struct DatabaseClient {
    pool: Pool<RedisConnectionManager>,
}

impl DatabaseClient {
    /// Build a Redis connection pool using bb8 + multiplexed connections.
    pub async fn new(redis_url: &str, pool_size: u32) -> Result<Self, &'static str> {
        // 1. Create the Manager from a URL — this will use
        //    client.get_multiplexed_async_connection() internally
        //    to yield a MultiplexedConnection. :contentReference[oaicite:1]{index=1}
        let manager = RedisConnectionManager::new(redis_url)
            .map_err(|_| "Invalid Redis URL")?;
        
        // 2. Build the bb8 pool
        let pool = Pool::builder()
            .max_size(pool_size)                            // tune for your QPS
            .connection_timeout(Duration::from_secs(5))      // fail fast on slow connect
            .idle_timeout(Some(Duration::from_secs(300)))    // close idle conns
            .build(manager)
            .await
            .map_err(|_| "Failed to build Redis pool")?;

        Ok(Self { pool })
    }

    /// Get a URL string by key, via a pooled multiplexed connection
    pub async fn get(&self, key: &str) -> Result<String, &'static str> {
        // Grab a multiplexed connection from the pool
        let mut conn = self.pool
            .get()
            .await
            .map_err(|_| "Failed to get Redis connection")?;

        // Perform a GET — uses `AsyncCommands::get` :contentReference[oaicite:2]{index=2}
        let data: Option<Vec<u8>> = conn
            .get(key)
            .await
            .map_err(|_| "Redis GET failed")?;

        data
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .ok_or("Key not found")
    }
}
