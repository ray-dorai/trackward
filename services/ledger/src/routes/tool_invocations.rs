use axum::extract::{Path, State};
use axum::Json;
use uuid::Uuid;

use crate::errors::Error;
use crate::models::{CreateToolInvocation, ToolInvocation};
use crate::AppState;

pub async fn create(
    State(state): State<AppState>,
    Json(input): Json<CreateToolInvocation>,
) -> Result<Json<ToolInvocation>, Error> {
    let id = Uuid::now_v7();
    let row = sqlx::query_as::<_, ToolInvocation>(
        "INSERT INTO tool_invocations
            (id, run_id, tool, input, output, status, status_code,
             started_at, finished_at, metadata)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         RETURNING *",
    )
    .bind(id)
    .bind(input.run_id)
    .bind(&input.tool)
    .bind(&input.input)
    .bind(&input.output)
    .bind(&input.status)
    .bind(input.status_code)
    .bind(input.started_at)
    .bind(input.finished_at)
    .bind(&input.metadata)
    .fetch_one(&state.db)
    .await?;

    tracing::info!(
        id = %row.id,
        run_id = %row.run_id,
        tool = %row.tool,
        status = %row.status,
        "tool_invocation recorded"
    );
    Ok(Json(row))
}

pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ToolInvocation>, Error> {
    let row =
        sqlx::query_as::<_, ToolInvocation>("SELECT * FROM tool_invocations WHERE id = $1")
            .bind(id)
            .fetch_optional(&state.db)
            .await?
            .ok_or(Error::NotFound)?;
    Ok(Json(row))
}

pub async fn list_for_run(
    State(state): State<AppState>,
    Path(run_id): Path<Uuid>,
) -> Result<Json<Vec<ToolInvocation>>, Error> {
    let rows = sqlx::query_as::<_, ToolInvocation>(
        "SELECT * FROM tool_invocations WHERE run_id = $1 ORDER BY started_at ASC",
    )
    .bind(run_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}
