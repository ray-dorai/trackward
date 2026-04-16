use axum::extract::{Path, State};
use axum::Json;
use uuid::Uuid;

use crate::errors::Error;
use crate::models::{CreateRun, Run};
use crate::AppState;

pub async fn create(
    State(state): State<AppState>,
    Json(input): Json<CreateRun>,
) -> Result<Json<Run>, Error> {
    let id = Uuid::now_v7();
    let run = sqlx::query_as::<_, Run>(
        "INSERT INTO runs (id, agent, started_at, metadata)
         VALUES ($1, $2, $3, $4)
         RETURNING *",
    )
    .bind(id)
    .bind(&input.agent)
    .bind(input.started_at)
    .bind(&input.metadata)
    .fetch_one(&state.db)
    .await?;

    tracing::info!(run_id = %run.id, agent = %run.agent, "run created");
    Ok(Json(run))
}

pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Run>, Error> {
    let run = sqlx::query_as::<_, Run>("SELECT * FROM runs WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(Error::NotFound)?;

    Ok(Json(run))
}

pub async fn list(State(state): State<AppState>) -> Result<Json<Vec<Run>>, Error> {
    let runs = sqlx::query_as::<_, Run>("SELECT * FROM runs ORDER BY created_at DESC LIMIT 100")
        .fetch_all(&state.db)
        .await?;

    Ok(Json(runs))
}
