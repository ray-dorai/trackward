use axum::extract::{Path, State};
use axum::Json;
use uuid::Uuid;

use crate::actor::Actor;
use crate::errors::Error;
use crate::models::{CreateGuardrail, Guardrail};
use crate::AppState;

pub async fn create(
    State(state): State<AppState>,
    actor: Actor,
    Json(input): Json<CreateGuardrail>,
) -> Result<Json<Guardrail>, Error> {
    let id = Uuid::now_v7();
    let row = sqlx::query_as::<_, Guardrail>(
        "INSERT INTO guardrails
            (id, run_id, name, stage, target, outcome, detail, evaluated_at, actor_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
         RETURNING *",
    )
    .bind(id)
    .bind(input.run_id)
    .bind(&input.name)
    .bind(&input.stage)
    .bind(input.target.as_deref())
    .bind(&input.outcome)
    .bind(&input.detail)
    .bind(input.evaluated_at)
    .bind(&actor.0)
    .fetch_one(&state.db)
    .await?;

    tracing::info!(
        id = %row.id,
        run_id = %row.run_id,
        name = %row.name,
        outcome = %row.outcome,
        "guardrail recorded"
    );
    Ok(Json(row))
}

pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Guardrail>, Error> {
    let row = sqlx::query_as::<_, Guardrail>("SELECT * FROM guardrails WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(Error::NotFound)?;
    Ok(Json(row))
}

pub async fn list_for_run(
    State(state): State<AppState>,
    Path(run_id): Path<Uuid>,
) -> Result<Json<Vec<Guardrail>>, Error> {
    let rows = sqlx::query_as::<_, Guardrail>(
        "SELECT * FROM guardrails WHERE run_id = $1 ORDER BY evaluated_at ASC",
    )
    .bind(run_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}
