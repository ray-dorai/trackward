use axum::extract::{Path, State};
use axum::Json;
use uuid::Uuid;

use crate::actor::Actor;
use crate::errors::Error;
use crate::models::{CreateRunVersionBinding, RunVersionBinding};
use crate::AppState;

pub async fn create(
    State(state): State<AppState>,
    actor: Actor,
    Path(run_id): Path<Uuid>,
    Json(input): Json<CreateRunVersionBinding>,
) -> Result<Json<RunVersionBinding>, Error> {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM runs WHERE id = $1)")
        .bind(run_id)
        .fetch_one(&state.db)
        .await?;
    if !exists {
        return Err(Error::NotFound);
    }

    // Bindings are write-once. A run that's already bound cannot be re-bound —
    // otherwise callers could silently swap the version history on a run.
    let row = sqlx::query_as::<_, RunVersionBinding>(
        "INSERT INTO run_version_bindings
            (run_id, prompt_version_id, policy_version_id, eval_result_id, actor_id)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING *",
    )
    .bind(run_id)
    .bind(input.prompt_version_id)
    .bind(input.policy_version_id)
    .bind(input.eval_result_id)
    .bind(&actor.0)
    .fetch_one(&state.db)
    .await
    .map_err(|e| match e {
        // Primary key collision (run already bound) — surface as a clean
        // 409 rather than a 500.
        sqlx::Error::Database(db_err) if db_err.is_unique_violation() => {
            Error::Conflict("run already bound to a version set".into())
        }
        other => Error::Db(other),
    })?;

    tracing::info!(
        run_id = %row.run_id,
        prompt_version_id = ?row.prompt_version_id,
        policy_version_id = ?row.policy_version_id,
        eval_result_id = ?row.eval_result_id,
        "run bound to versions"
    );
    Ok(Json(row))
}

pub async fn get(
    State(state): State<AppState>,
    Path(run_id): Path<Uuid>,
) -> Result<Json<RunVersionBinding>, Error> {
    let row = sqlx::query_as::<_, RunVersionBinding>(
        "SELECT * FROM run_version_bindings WHERE run_id = $1",
    )
    .bind(run_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(Error::NotFound)?;
    Ok(Json(row))
}
