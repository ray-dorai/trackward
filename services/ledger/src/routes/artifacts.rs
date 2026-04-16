use axum::body::Body;
use axum::extract::{Multipart, Path, State};
use axum::http::{header, StatusCode};
use axum::response::Response;
use axum::Json;
use uuid::Uuid;

use crate::errors::Error;
use crate::hash::sha256_hex;
use crate::models::Artifact;
use crate::AppState;

/// Upload an artifact as multipart form data.
/// Fields: run_id, label (optional), media_type (optional), metadata (optional JSON), file (binary).
pub async fn upload(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<Artifact>, Error> {
    let mut run_id: Option<Uuid> = None;
    let mut label = String::new();
    let mut media_type = "application/octet-stream".to_string();
    let mut file_data: Option<Vec<u8>> = None;
    let mut metadata = serde_json::Value::Object(serde_json::Map::new());

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "run_id" => {
                let text = field.text().await.map_err(|e| Error::BadRequest(e.to_string()))?;
                run_id = Some(
                    text.parse::<Uuid>()
                        .map_err(|e| Error::BadRequest(format!("invalid run_id: {e}")))?,
                );
            }
            "label" => {
                label = field.text().await.map_err(|e| Error::BadRequest(e.to_string()))?;
            }
            "media_type" => {
                media_type = field.text().await.map_err(|e| Error::BadRequest(e.to_string()))?;
            }
            "metadata" => {
                let text = field.text().await.map_err(|e| Error::BadRequest(e.to_string()))?;
                metadata = serde_json::from_str(&text)
                    .map_err(|e| Error::BadRequest(format!("invalid metadata JSON: {e}")))?;
            }
            "file" => {
                file_data = Some(
                    field
                        .bytes()
                        .await
                        .map_err(|e| Error::BadRequest(e.to_string()))?
                        .to_vec(),
                );
            }
            _ => {}
        }
    }

    let run_id = run_id.ok_or_else(|| Error::BadRequest("missing run_id".into()))?;
    let data = file_data.ok_or_else(|| Error::BadRequest("missing file".into()))?;

    let digest = sha256_hex(&data);
    let size = data.len() as i64;

    // Store blob in S3 (content-addressed)
    state.blob_store.put(&digest, data).await?;

    let id = Uuid::now_v7();
    let artifact = sqlx::query_as::<_, Artifact>(
        "INSERT INTO artifacts (id, run_id, sha256, size_bytes, media_type, label, metadata)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         RETURNING *",
    )
    .bind(id)
    .bind(run_id)
    .bind(&digest)
    .bind(size)
    .bind(&media_type)
    .bind(&label)
    .bind(&metadata)
    .fetch_one(&state.db)
    .await?;

    tracing::info!(
        artifact_id = %artifact.id,
        run_id = %run_id,
        sha256 = %digest,
        size_bytes = size,
        "artifact stored"
    );
    Ok(Json(artifact))
}

pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Artifact>, Error> {
    let artifact = sqlx::query_as::<_, Artifact>("SELECT * FROM artifacts WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(Error::NotFound)?;

    Ok(Json(artifact))
}

/// Download the raw bytes of an artifact. Re-verifies the SHA-256 on the way out.
pub async fn download(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Response, Error> {
    let artifact = sqlx::query_as::<_, Artifact>("SELECT * FROM artifacts WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(Error::NotFound)?;

    let data = state.blob_store.get(&artifact.sha256).await?;

    // Integrity check: whatever we fetched from blob storage must still hash
    // to what we recorded. If it doesn't, blob storage has been tampered with.
    let actual = sha256_hex(&data);
    if actual != artifact.sha256 {
        return Err(Error::HashMismatch {
            expected: artifact.sha256,
            actual,
        });
    }

    let response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, artifact.media_type)
        .header("x-sha256", artifact.sha256)
        .body(Body::from(data))
        .map_err(|e| Error::S3(e.to_string()))?;

    Ok(response)
}

pub async fn list_for_run(
    State(state): State<AppState>,
    Path(run_id): Path<Uuid>,
) -> Result<Json<Vec<Artifact>>, Error> {
    let artifacts = sqlx::query_as::<_, Artifact>(
        "SELECT * FROM artifacts WHERE run_id = $1 ORDER BY created_at ASC",
    )
    .bind(run_id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(artifacts))
}
