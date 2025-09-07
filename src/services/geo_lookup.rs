use once_cell::sync::OnceCell;
use dashmap::DashMap;
use std::{
    net::IpAddr,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::time;
use maxminddb::{geoip2::City, Reader};
use serde::{Deserialize, Serialize};

use crate::{
    config::settings::Settings,
    errors::AppError,
    services::{metrics, storage::sled::SledStorage, storage::storage::Storage},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeoLocation {
    pub continent_code: Option<String>,
    pub country_iso: Option<String>,
    pub city_name: Option<String>,
    pub postal_code: Option<String>,
    pub timezone: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
}

static GEOIP_READER: OnceCell<Arc<Reader<Vec<u8>>>> = OnceCell::new();
static HOT_CACHE: OnceCell<Arc<DashMap<IpAddr, (GeoLocation, Instant)>>> = OnceCell::new();
static SLED_GEO: OnceCell<Arc<SledStorage>> = OnceCell::new();
static GEO_TTL: OnceCell<Duration> = OnceCell::new();
static EVICT_INTERVAL: OnceCell<Duration> = OnceCell::new();

pub fn init_geo_lookup(settings: &Settings) -> Result<(), AppError> {
    GEOIP_READER.get_or_init(|| {
        Arc::new(
            Reader::open_readfile(&settings.cache.geoip_mmdb_path)
                .map_err(|e| AppError::Internal(format!("Failed to open GeoIP DB: {}", e)))
                .unwrap(),
        )
    });

    HOT_CACHE.get_or_init(|| Arc::new(DashMap::with_capacity(settings.cache.geo_hot_capacity)));
    SLED_GEO.get_or_init(|| Arc::new(SledStorage::new(settings)));
    GEO_TTL.get_or_init(|| Duration::from_secs(settings.cache.geo_ttl_seconds));
    EVICT_INTERVAL.get_or_init(|| Duration::from_secs(settings.cache.geo_evict_interval_secs));

    let hot_cache = HOT_CACHE.get().unwrap().clone();
    let ttl = *GEO_TTL.get().unwrap();
    let interval = *EVICT_INTERVAL.get().unwrap();

    tokio::spawn(async move {
        let mut ticker = time::interval(interval);
        loop {
            ticker.tick().await;
            let now = Instant::now();
            let initial_len = hot_cache.len();
            hot_cache.retain(|_, &mut (_, inserted)| now.duration_since(inserted) < ttl);
            let evicted = initial_len - hot_cache.len();
            metrics::record_cache_eviction("geo_hot", evicted as u64);
            tracing::debug!("Evicted {} geo cache entries in {:?}", evicted, now.elapsed());
        }
    });

    Ok(())
}

pub async fn lookup_geo(ip: IpAddr) -> Result<Option<GeoLocation>, AppError> {
    let start_total = Instant::now();

    // 1. Hot cache
    if let Some(mut entry) = HOT_CACHE.get().unwrap().get_mut(&ip) {
        entry.value_mut().1 = Instant::now();
        metrics::record_cache_hit("geo_hot", start_total);
        return Ok(Some(entry.value().0.clone()));
    }

    // 2. Sled storage
    let sled_start = Instant::now();
    let sled = SLED_GEO.get().unwrap();
    if let Ok(serialized) = sled.get(&ip.to_string()).await {
        if !serialized.is_empty() {
            if let Ok(loc) = serde_json::from_str::<GeoLocation>(&serialized) {
                metrics::record_db_latency("get_geo_sled", sled_start);
                HOT_CACHE.get().unwrap().insert(ip, (loc.clone(), Instant::now()));
                metrics::record_cache_hit("geo_sled", start_total);
                return Ok(Some(loc));
            } else {
                tracing::warn!("Failed to deserialize Sled geo data for {}", ip);
            }
        }
        metrics::record_db_latency("get_geo_sled", sled_start);
    } else {
        metrics::record_db_latency("get_geo_sled", sled_start);
        tracing::warn!("Sled get error for geo lookup {}", ip);
    }

    // 3. MaxMind lookup
    let mm_start = Instant::now();
    let reader = GEOIP_READER.get().unwrap();
    let geo_opt = reader
        .lookup::<City>(ip)?
        .map(|record| GeoLocation {
            continent_code: record.continent.and_then(|c| c.code).map(String::from),
            country_iso: record.country.and_then(|c| c.iso_code).map(String::from),
            city_name: record
                .city
                .and_then(|c| c.names)
                .and_then(|names| names.get("en").cloned())
                .map(String::from),
            postal_code: record.postal.and_then(|p| p.code).map(String::from),
            timezone: record.location.as_ref().and_then(|l| l.time_zone).map(String::from),
            latitude: record.location.as_ref().and_then(|l| l.latitude),
            longitude: record.location.as_ref().and_then(|l| l.longitude),
        });
    metrics::record_db_latency("lookup_geo_maxmind", mm_start);

    // Cache results
    if let Some(ref loc) = geo_opt {
        let sled_set_start = Instant::now();
        if let Ok(serialized) = serde_json::to_string(loc) {
            let ttl_secs = GEO_TTL.get().unwrap().as_secs();
            if let Err(e) = sled.set_ex(&ip.to_string(), &serialized, ttl_secs).await {
                tracing::warn!("Failed to set Sled geo data for {}: {}", ip, e);
            }
            metrics::record_db_latency("set_geo_sled", sled_set_start);
        } else {
            tracing::warn!("Failed to serialize geo data for {}", ip);
        }
        HOT_CACHE.get().unwrap().insert(ip, (loc.clone(), Instant::now()));
    }

    metrics::record_cache_latency("geo_total", start_total);
    Ok(geo_opt)
}
