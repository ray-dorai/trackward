use axum::extract::{Path, State};
use axum::Json;
use uuid::Uuid;

use crate::errors::Error;
use crate::models::{BiasSlice, CreateBiasSlice};
use crate::AppState;

pub async fn create(
    State(state): State<AppState>,
    Json(input): Json<CreateBiasSlice>,
) -> Result<Json<BiasSlice>, Error> {
    // The CHECK constraint catches (NULL, NULL) at the DB layer — reject it
    // up front with a cleaner 400 so callers aren't chasing a 500.
    if input.run_id.is_none() && input.eval_result_id.is_none() {
        return Err(Error::BadRequest(
            "bias_slice requires either run_id or eval_result_id".into(),
        ));
    }

    let id = Uuid::now_v7();
    let row = sqlx::query_as::<_, BiasSlice>(
        "INSERT INTO bias_slices
            (id, run_id, eval_result_id, label, value, score, metadata)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         RETURNING *",
    )
    .bind(id)
    .bind(input.run_id)
    .bind(input.eval_result_id)
    .bind(&input.label)
    .bind(input.value.as_deref())
    .bind(input.score)
    .bind(&input.metadata)
    .fetch_one(&state.db)
    .await?;

    tracing::info!(
        id = %row.id,
        run_id = ?row.run_id,
        eval_result_id = ?row.eval_result_id,
        label = %row.label,
        "bias_slice recorded"
    );
    Ok(Json(row))
}

pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<BiasSlice>, Error> {
    let row = sqlx::query_as::<_, BiasSlice>("SELECT * FROM bias_slices WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(Error::NotFound)?;
    Ok(Json(row))
}

pub async fn list_for_run(
    State(state): State<AppState>,
    Path(run_id): Path<Uuid>,
) -> Result<Json<Vec<BiasSlice>>, Error> {
    let rows = sqlx::query_as::<_, BiasSlice>(
        "SELECT * FROM bias_slices WHERE run_id = $1 ORDER BY created_at ASC",
    )
    .bind(run_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}
