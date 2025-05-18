use mini_redis::client::{self, Client};

pub struct DatabaseClient {
    client: Client,
}

impl DatabaseClient {
    /// Create a new DatabaseClient connected to the given address
    pub async fn new(address: &str) -> Result<Self, &'static str> {
        let client = client::connect(address).await.map_err(|_| "Failed to connect to Redis")?;
        Ok(Self { client })
    }

    /// Get a value by key from Redis
    pub async fn get(&mut self, key: &str) -> Result<String, &'static str> {
             self.client
            .get(key)
            .await
            .map_err(|_| "Failed to get value")?
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .ok_or("Key not found")
    }
}