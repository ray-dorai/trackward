use axum::extract::{Path, State};
use axum::Json;
use uuid::Uuid;

use crate::errors::Error;
use crate::models::{CreateHumanApproval, HumanApproval};
use crate::AppState;

pub async fn create(
    State(state): State<AppState>,
    Json(input): Json<CreateHumanApproval>,
) -> Result<Json<HumanApproval>, Error> {
    let row = sqlx::query_as::<_, HumanApproval>(
        "INSERT INTO human_approvals
            (id, run_id, tool, decision, reason, decided_by,
             requested_at, decided_at, metadata)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
         RETURNING *",
    )
    .bind(input.id)
    .bind(input.run_id)
    .bind(&input.tool)
    .bind(&input.decision)
    .bind(input.reason.as_deref())
    .bind(input.decided_by.as_deref())
    .bind(input.requested_at)
    .bind(input.decided_at)
    .bind(&input.metadata)
    .fetch_one(&state.db)
    .await
    .map_err(|e| match e {
        // id collision (approval already recorded) — keep it a conflict
        // rather than a 500.
        sqlx::Error::Database(db_err) if db_err.is_unique_violation() => {
            Error::Conflict("human_approval already recorded".into())
        }
        other => Error::Db(other),
    })?;

    tracing::info!(
        id = %row.id,
        run_id = %row.run_id,
        tool = %row.tool,
        decision = %row.decision,
        "human_approval recorded"
    );
    Ok(Json(row))
}

pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<HumanApproval>, Error> {
    let row = sqlx::query_as::<_, HumanApproval>("SELECT * FROM human_approvals WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(Error::NotFound)?;
    Ok(Json(row))
}
