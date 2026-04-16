use aws_sdk_s3::config::{BehaviorVersion, Credentials, Region};
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::Client;

use crate::config::Config;
use crate::errors::Error;

#[derive(Clone)]
pub struct BlobStore {
    client: Client,
    bucket: String,
}

impl BlobStore {
    pub async fn new(config: &Config) -> Self {
        let client = if let Some(endpoint) = &config.s3_endpoint {
            // Local dev (MinIO): endpoint + static credentials from env.
            let access_key = std::env::var("AWS_ACCESS_KEY_ID")
                .unwrap_or_else(|_| "minioadmin".into());
            let secret_key = std::env::var("AWS_SECRET_ACCESS_KEY")
                .unwrap_or_else(|_| "minioadmin".into());
            let creds = Credentials::new(access_key, secret_key, None, None, "trackward-dev");
            let s3_config = aws_sdk_s3::Config::builder()
                .behavior_version(BehaviorVersion::latest())
                .region(Region::new(config.s3_region.clone()))
                .endpoint_url(endpoint)
                .credentials_provider(creds)
                .force_path_style(true)
                .build();
            Client::from_conf(s3_config)
        } else {
            // Production: pick up creds from the AWS default chain (IRSA, EC2 role, env).
            let aws_config = aws_config::defaults(BehaviorVersion::latest())
                .region(Region::new(config.s3_region.clone()))
                .load()
                .await;
            Client::new(&aws_config)
        };

        Self {
            client,
            bucket: config.s3_bucket.clone(),
        }
    }

    /// Store a blob by its content-addressed SHA-256 hex key.
    pub async fn put(&self, sha256_hex: &str, data: Vec<u8>) -> Result<(), Error> {
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(sha256_hex)
            .body(ByteStream::from(data))
            .content_type("application/octet-stream")
            .send()
            .await
            .map_err(|e| Error::S3(format!("{e:?}")))?;

        Ok(())
    }

    /// Retrieve a blob by SHA-256 hex key.
    pub async fn get(&self, sha256_hex: &str) -> Result<Vec<u8>, Error> {
        let resp = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(sha256_hex)
            .send()
            .await
            .map_err(|e| Error::S3(format!("{e:?}")))?;

        let bytes = resp
            .body
            .collect()
            .await
            .map_err(|e| Error::S3(format!("{e:?}")))?
            .into_bytes();

        Ok(bytes.to_vec())
    }
}
