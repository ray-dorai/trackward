use axum::extract::{Path, State};
use axum::Json;
use uuid::Uuid;

use crate::errors::Error;
use crate::models::{Case, CreateCase};
use crate::AppState;

pub async fn create(
    State(state): State<AppState>,
    Json(input): Json<CreateCase>,
) -> Result<Json<Case>, Error> {
    let id = Uuid::now_v7();
    let row = sqlx::query_as::<_, Case>(
        "INSERT INTO cases
            (id, title, description, opened_by, opened_at, metadata)
         VALUES ($1, $2, $3, $4, $5, $6)
         RETURNING *",
    )
    .bind(id)
    .bind(&input.title)
    .bind(&input.description)
    .bind(&input.opened_by)
    .bind(input.opened_at)
    .bind(&input.metadata)
    .fetch_one(&state.db)
    .await?;

    tracing::info!(
        id = %row.id,
        opened_by = %row.opened_by,
        "case opened"
    );
    Ok(Json(row))
}

pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Case>, Error> {
    let row = sqlx::query_as::<_, Case>("SELECT * FROM cases WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(Error::NotFound)?;
    Ok(Json(row))
}
