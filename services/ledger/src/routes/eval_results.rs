use axum::extract::{Path, Query, State};
use axum::Json;
use serde::Deserialize;
use uuid::Uuid;

use crate::errors::Error;
use crate::models::{CreateEvalResult, EvalResult};
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub workflow: Option<String>,
    pub version: Option<String>,
    pub prompt_version_id: Option<Uuid>,
}

pub async fn create(
    State(state): State<AppState>,
    Json(input): Json<CreateEvalResult>,
) -> Result<Json<EvalResult>, Error> {
    let id = Uuid::now_v7();
    let row = sqlx::query_as::<_, EvalResult>(
        "INSERT INTO eval_results
            (id, workflow, version, prompt_version_id, git_sha, content_hash, passed, summary, ran_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
         RETURNING *",
    )
    .bind(id)
    .bind(&input.workflow)
    .bind(&input.version)
    .bind(input.prompt_version_id)
    .bind(&input.git_sha)
    .bind(&input.content_hash)
    .bind(input.passed)
    .bind(&input.summary)
    .bind(input.ran_at)
    .fetch_one(&state.db)
    .await?;

    tracing::info!(
        id = %row.id,
        workflow = %row.workflow,
        version = %row.version,
        passed = row.passed,
        "eval_result recorded"
    );
    Ok(Json(row))
}

pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<EvalResult>, Error> {
    let row = sqlx::query_as::<_, EvalResult>("SELECT * FROM eval_results WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(Error::NotFound)?;
    Ok(Json(row))
}

pub async fn list(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<EvalResult>>, Error> {
    let rows = sqlx::query_as::<_, EvalResult>(
        "SELECT * FROM eval_results
         WHERE ($1::text IS NULL OR workflow = $1)
           AND ($2::text IS NULL OR version = $2)
           AND ($3::uuid IS NULL OR prompt_version_id = $3)
         ORDER BY ran_at DESC
         LIMIT 100",
    )
    .bind(q.workflow.as_deref())
    .bind(q.version.as_deref())
    .bind(q.prompt_version_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}
