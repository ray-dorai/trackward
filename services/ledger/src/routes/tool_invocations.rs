use axum::extract::{Path, State};
use axum::Json;
use chain_core::CanonicalField;
use uuid::Uuid;

use crate::actor::Actor;
use crate::chain::{compute_chain, ChainTable};
use crate::errors::Error;
use crate::models::{CreateToolInvocation, ToolInvocation};
use crate::AppState;

pub async fn create(
    State(state): State<AppState>,
    actor: Actor,
    Json(input): Json<CreateToolInvocation>,
) -> Result<Json<ToolInvocation>, Error> {
    let id = Uuid::now_v7();
    let status_code_i64 = input.status_code.map(|c| c as i64);

    let mut tx = state.db.begin().await?;

    let fields = vec![
        CanonicalField::uuid("id", id),
        CanonicalField::uuid("run_id", input.run_id),
        CanonicalField::str("tool", &input.tool),
        CanonicalField::json("input", input.input.clone()),
        CanonicalField::json("output", input.output.clone()),
        CanonicalField::str("status", &input.status),
        CanonicalField::opt_i64("status_code", status_code_i64),
        CanonicalField::timestamp("started_at", input.started_at),
        CanonicalField::timestamp("finished_at", input.finished_at),
        CanonicalField::json("metadata", input.metadata.clone()),
        CanonicalField::str("actor_id", &actor.0),
    ];
    let link = compute_chain(&mut tx, ChainTable::ToolInvocations, input.run_id, &fields).await?;

    let row = sqlx::query_as::<_, ToolInvocation>(
        "INSERT INTO tool_invocations
            (id, run_id, tool, input, output, status, status_code,
             started_at, finished_at, metadata, actor_id, prev_hash, row_hash)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
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
    .bind(&actor.0)
    .bind(link.prev_hash.as_deref())
    .bind(&link.row_hash)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;

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
