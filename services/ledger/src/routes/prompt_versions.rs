use axum::extract::{Path, Query, State};
use axum::Json;
use serde::Deserialize;
use uuid::Uuid;

use crate::actor::Actor;
use crate::errors::Error;
use crate::models::{CreatePromptVersion, PromptVersion};
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub workflow: Option<String>,
    pub version: Option<String>,
    pub content_hash: Option<String>,
}

pub async fn create(
    State(state): State<AppState>,
    actor: Actor,
    Json(input): Json<CreatePromptVersion>,
) -> Result<Json<PromptVersion>, Error> {
    // (workflow, version, content_hash) is unique — re-registering the
    // same content returns the existing row instead of erroring so gateway
    // startup is idempotent.
    if let Some(existing) = sqlx::query_as::<_, PromptVersion>(
        "SELECT * FROM prompt_versions
         WHERE workflow = $1 AND version = $2 AND content_hash = $3",
    )
    .bind(&input.workflow)
    .bind(&input.version)
    .bind(&input.content_hash)
    .fetch_optional(&state.db)
    .await?
    {
        return Ok(Json(existing));
    }

    let id = Uuid::now_v7();
    let row = sqlx::query_as::<_, PromptVersion>(
        "INSERT INTO prompt_versions (id, workflow, version, git_sha, content_hash, metadata, actor_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         RETURNING *",
    )
    .bind(id)
    .bind(&input.workflow)
    .bind(&input.version)
    .bind(&input.git_sha)
    .bind(&input.content_hash)
    .bind(&input.metadata)
    .bind(&actor.0)
    .fetch_one(&state.db)
    .await?;

    tracing::info!(
        id = %row.id,
        workflow = %row.workflow,
        version = %row.version,
        content_hash = %row.content_hash,
        "prompt_version registered"
    );
    Ok(Json(row))
}

pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<PromptVersion>, Error> {
    let row = sqlx::query_as::<_, PromptVersion>("SELECT * FROM prompt_versions WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(Error::NotFound)?;
    Ok(Json(row))
}

pub async fn list(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<PromptVersion>>, Error> {
    let rows = sqlx::query_as::<_, PromptVersion>(
        "SELECT * FROM prompt_versions
         WHERE ($1::text IS NULL OR workflow = $1)
           AND ($2::text IS NULL OR version = $2)
           AND ($3::text IS NULL OR content_hash = $3)
         ORDER BY created_at DESC
         LIMIT 100",
    )
    .bind(q.workflow.as_deref())
    .bind(q.version.as_deref())
    .bind(q.content_hash.as_deref())
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}
