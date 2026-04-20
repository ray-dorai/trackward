use std::env;

#[derive(Clone, Debug)]
pub struct Config {
    pub database_url: String,
    pub listen_addr: String,
    pub s3_bucket: String,
    pub s3_endpoint: Option<String>,
    pub s3_region: String,
    /// Phase 9b merkle anchor config. `None` means "no anchor loop"
    /// (development/test default); when set, `main.rs` spawns the
    /// periodic anchor task.
    pub anchor: Option<AnchorConfig>,
}

/// Tuning knobs for the merkle-anchor loop. All environment variables
/// must be supplied together — we deliberately won't silently default
/// `ANCHOR_BUCKET` since a misconfigured deploy would then anchor into
/// the artifact bucket, which is the wrong durability contract.
#[derive(Clone, Debug)]
pub struct AnchorConfig {
    /// WORM bucket name that receives signed manifests. Must be distinct
    /// from `s3_bucket` in production.
    pub bucket: String,
    /// Local-dev endpoint override (MinIO). Mirrors `Config::s3_endpoint`.
    pub s3_endpoint: Option<String>,
    pub s3_region: String,
    /// Seconds between anchor ticks. Typical production: 60–300.
    pub interval_secs: u64,
    /// Object-lock retention, in days. The bucket should be configured
    /// with a default retention at least this long; this header is the
    /// per-object reinforcement.
    pub retain_days: u32,
}

impl Config {
    pub fn from_env() -> Self {
        let anchor = match env::var("ANCHOR_BUCKET").ok() {
            Some(bucket) if !bucket.is_empty() => Some(AnchorConfig {
                bucket,
                s3_endpoint: env::var("ANCHOR_S3_ENDPOINT")
                    .ok()
                    .or_else(|| env::var("S3_ENDPOINT").ok()),
                s3_region: env::var("ANCHOR_S3_REGION")
                    .unwrap_or_else(|_| {
                        env::var("S3_REGION").unwrap_or_else(|_| "us-east-1".into())
                    }),
                interval_secs: env::var("ANCHOR_INTERVAL_SECS")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(60),
                retain_days: env::var("ANCHOR_RETAIN_DAYS")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(365 * 7),
            }),
            _ => None,
        };

        Self {
            database_url: env::var("DATABASE_URL").unwrap_or_else(|_| {
                "postgres://trackward:trackward@localhost:5432/trackward?sslmode=disable".into()
            }),
            listen_addr: env::var("LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:3000".into()),
            s3_bucket: env::var("S3_BUCKET").unwrap_or_else(|_| "trackward-artifacts".into()),
            s3_endpoint: env::var("S3_ENDPOINT").ok(),
            s3_region: env::var("S3_REGION").unwrap_or_else(|_| "us-east-1".into()),
            anchor,
        }
    }
}
