use std::env;

#[derive(Clone, Debug)]
pub struct Config {
    pub database_url: String,
    pub listen_addr: String,
    pub s3_bucket: String,
    pub s3_endpoint: Option<String>,
    pub s3_region: String,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            database_url: env::var("DATABASE_URL").unwrap_or_else(|_| {
                "postgres://trackward:trackward@localhost:5432/trackward?sslmode=disable".into()
            }),
            listen_addr: env::var("LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:3000".into()),
            s3_bucket: env::var("S3_BUCKET").unwrap_or_else(|_| "trackward-artifacts".into()),
            s3_endpoint: env::var("S3_ENDPOINT").ok(),
            s3_region: env::var("S3_REGION").unwrap_or_else(|_| "us-east-1".into()),
        }
    }
}
