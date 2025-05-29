use chrono::{DateTime, Utc};
pub trait Clock {
    fn now(&self) -> DateTime<Utc>;
}

pub struct SystemClock;
impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

#[cfg(test)]
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
}
