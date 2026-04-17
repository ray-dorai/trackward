use axum::extract::{Path, Query, State};
use axum::Json;
use serde::Deserialize;
use uuid::Uuid;

use crate::errors::Error;
use crate::models::{CreatePolicyVersion, PolicyVersion};
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub scope: Option<String>,
    pub version: Option<String>,
    pub content_hash: Option<String>,
}

pub async fn create(
    State(state): State<AppState>,
    Json(input): Json<CreatePolicyVersion>,
) -> Result<Json<PolicyVersion>, Error> {
    if let Some(existing) = sqlx::query_as::<_, PolicyVersion>(
        "SELECT * FROM policy_versions
         WHERE scope = $1 AND version = $2 AND content_hash = $3",
    )
    .bind(&input.scope)
    .bind(&input.version)
    .bind(&input.content_hash)
    .fetch_optional(&state.db)
    .await?
    {
        return Ok(Json(existing));
    }

    let id = Uuid::now_v7();
    let row = sqlx::query_as::<_, PolicyVersion>(
        "INSERT INTO policy_versions (id, scope, version, git_sha, content_hash, metadata)
         VALUES ($1, $2, $3, $4, $5, $6)
         RETURNING *",
    )
    .bind(id)
    .bind(&input.scope)
    .bind(&input.version)
    .bind(&input.git_sha)
    .bind(&input.content_hash)
    .bind(&input.metadata)
    .fetch_one(&state.db)
    .await?;

    tracing::info!(
        id = %row.id,
        scope = %row.scope,
        version = %row.version,
        "policy_version registered"
    );
    Ok(Json(row))
}

pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<PolicyVersion>, Error> {
    let row = sqlx::query_as::<_, PolicyVersion>("SELECT * FROM policy_versions WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(Error::NotFound)?;
    Ok(Json(row))
}

pub async fn list(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<PolicyVersion>>, Error> {
    let rows = sqlx::query_as::<_, PolicyVersion>(
        "SELECT * FROM policy_versions
         WHERE ($1::text IS NULL OR scope = $1)
           AND ($2::text IS NULL OR version = $2)
           AND ($3::text IS NULL OR content_hash = $3)
         ORDER BY created_at DESC
         LIMIT 100",
    )
    .bind(q.scope.as_deref())
    .bind(q.version.as_deref())
    .bind(q.content_hash.as_deref())
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}
