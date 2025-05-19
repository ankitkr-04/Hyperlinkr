use std::sync::Arc;

use tokio::sync::Mutex;
use sled::Db;


pub struct SledStorage {
    db: Arc<Mutex<Db>>,
}

impl SledStorage {
    pub fn new(path: &str) -> Self {
        let db = sled::open(path).unwrap();
        Self {
            db: Arc::new(Mutex::new(db)),
        }
    }

    pub async fn store(&self, key: &str, value: &str) {
        let db = self.db.lock().await;
        let _ = db.insert(key, value.as_bytes());
    }

    pub async fn get(&self, key: &str) -> Option<String> {
        let db = self.db.lock().await;
        db.get(key)
            .unwrap()
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
    }
}