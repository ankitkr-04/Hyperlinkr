use chrono::{DateTime, Utc};

pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
    fn clone(&self) -> Self
    where
        Self: Sized;
}

#[derive(Clone)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }

    fn clone(&self) -> Self {
        SystemClock
    }
}

#[cfg(test)]
#[derive(Clone)]
pub struct MockClock(chrono::DateTime<chrono::Utc>);

#[cfg(test)]
impl MockClock {
    pub fn new(time: chrono::DateTime<chrono::Utc>) -> Self {
        Self(time)
    }
}

#[cfg(test)]
impl Clock for MockClock {
    fn now(&self) -> chrono::DateTime<chrono::Utc> {
        self.0
    }

    fn clone(&self) -> Self {
        MockClock(self.0)
    }
}