//! S3 object-lock (WORM) anchor sink.
//!
//! Every manifest goes to a **separate** bucket from the artifact blob
//! store. The anchor bucket is expected to have object-lock enabled in
//! compliance mode with a retention policy on write; we don't configure
//! the bucket here (that's a deploy-time responsibility) but we *do*
//! set `object_lock_mode=COMPLIANCE` and `object_lock_retain_until_date`
//! on every put so the bucket owner itself cannot silently delete an
//! anchor during the retention window.
//!
//! The sink is deliberately dumb: one method, one bucket, one role —
//! same pattern as `BlobStore`. If we ever need a second WORM sink
//! (e.g., Glacier, GCS), it's a new `AnchorSink` variant, not a
//! generalization of this type.

use std::time::Duration;

use aws_sdk_s3::config::{BehaviorVersion, Credentials, Region};
use aws_sdk_s3::primitives::{ByteStream, DateTime as SdkDateTime};
use aws_sdk_s3::types::ObjectLockMode;
use aws_sdk_s3::Client;
use chrono::Utc;

use crate::config::AnchorConfig;
use crate::errors::Error;

#[derive(Clone)]
pub struct S3Sink {
    client: Client,
    bucket: String,
    retain_days: u32,
}

impl S3Sink {
    pub async fn new(cfg: &AnchorConfig) -> Self {
        let client = if let Some(endpoint) = &cfg.s3_endpoint {
            let access_key =
                std::env::var("AWS_ACCESS_KEY_ID").unwrap_or_else(|_| "minioadmin".into());
            let secret_key = std::env::var("AWS_SECRET_ACCESS_KEY")
                .unwrap_or_else(|_| "minioadmin".into());
            let creds = Credentials::new(access_key, secret_key, None, None, "trackward-dev");
            let s3_config = aws_sdk_s3::Config::builder()
                .behavior_version(BehaviorVersion::latest())
                .region(Region::new(cfg.s3_region.clone()))
                .endpoint_url(endpoint)
                .credentials_provider(creds)
                .force_path_style(true)
                .build();
            Client::from_conf(s3_config)
        } else {
            let aws_config = aws_config::defaults(BehaviorVersion::latest())
                .region(Region::new(cfg.s3_region.clone()))
                .load()
                .await;
            Client::new(&aws_config)
        };

        Self {
            client,
            bucket: cfg.bucket.clone(),
            retain_days: cfg.retain_days,
        }
    }

    pub async fn put(&self, key: &str, bytes: Vec<u8>) -> Result<String, Error> {
        let retain_until = Utc::now() + Duration::from_secs(u64::from(self.retain_days) * 86_400);
        let retain_until_sdk = SdkDateTime::from_secs(retain_until.timestamp());

        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .body(ByteStream::from(bytes))
            .content_type("application/json")
            .object_lock_mode(ObjectLockMode::Compliance)
            .object_lock_retain_until_date(retain_until_sdk)
            .send()
            .await
            .map_err(|e| Error::S3(format!("{e:?}")))?;

        Ok(format!("s3://{}/{}", self.bucket, key))
    }
}
