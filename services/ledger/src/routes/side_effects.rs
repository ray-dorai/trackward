use axum::extract::{Path, State};
use axum::Json;
use uuid::Uuid;

use crate::actor::Actor;
use crate::errors::Error;
use crate::models::{CreateSideEffect, SideEffect};
use crate::AppState;

pub async fn create(
    State(state): State<AppState>,
    actor: Actor,
    Json(input): Json<CreateSideEffect>,
) -> Result<Json<SideEffect>, Error> {
    let id = Uuid::now_v7();
    let row = sqlx::query_as::<_, SideEffect>(
        "INSERT INTO side_effects
            (id, run_id, tool_invocation_id, kind, target, status, confirmation, observed_at, actor_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
         RETURNING *",
    )
    .bind(id)
    .bind(input.run_id)
    .bind(input.tool_invocation_id)
    .bind(&input.kind)
    .bind(&input.target)
    .bind(&input.status)
    .bind(&input.confirmation)
    .bind(input.observed_at)
    .bind(&actor.0)
    .fetch_one(&state.db)
    .await?;

    tracing::info!(
        id = %row.id,
        run_id = %row.run_id,
        kind = %row.kind,
        status = %row.status,
        "side_effect recorded"
    );
    Ok(Json(row))
}

pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<SideEffect>, Error> {
    let row = sqlx::query_as::<_, SideEffect>("SELECT * FROM side_effects WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(Error::NotFound)?;
    Ok(Json(row))
}

pub async fn list_for_run(
    State(state): State<AppState>,
    Path(run_id): Path<Uuid>,
) -> Result<Json<Vec<SideEffect>>, Error> {
    let rows = sqlx::query_as::<_, SideEffect>(
        "SELECT * FROM side_effects WHERE run_id = $1 ORDER BY observed_at ASC",
    )
    .bind(run_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}
