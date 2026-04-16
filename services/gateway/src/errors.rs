use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("unknown tool: {0}")]
    UnknownTool(String),

    #[error("ledger request failed: {0}")]
    Ledger(String),

    #[error("backend request failed: {0}")]
    Backend(String),

    #[error("bad request: {0}")]
    BadRequest(String),

    #[error("not found")]
    NotFound,

    #[error("internal error: {0}")]
    Internal(String),
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let status = match &self {
            Error::UnknownTool(_) => StatusCode::NOT_FOUND,
            Error::NotFound => StatusCode::NOT_FOUND,
            Error::BadRequest(_) => StatusCode::BAD_REQUEST,
            Error::Ledger(_) | Error::Backend(_) | Error::Internal(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        };
        let body = serde_json::json!({ "error": self.to_string() });
        (status, axum::Json(body)).into_response()
    }
}
