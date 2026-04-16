use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("not found")]
    NotFound,

    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),

    #[error("s3 error: {0}")]
    S3(String),

    #[error("hash mismatch: expected {expected}, got {actual}")]
    HashMismatch { expected: String, actual: String },

    #[error("bad request: {0}")]
    BadRequest(String),
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let status = match &self {
            Error::NotFound => StatusCode::NOT_FOUND,
            Error::Db(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Error::S3(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Error::HashMismatch { .. } => StatusCode::CONFLICT,
            Error::BadRequest(_) => StatusCode::BAD_REQUEST,
        };
        let body = serde_json::json!({ "error": self.to_string() });
        (status, axum::Json(body)).into_response()
    }
}
