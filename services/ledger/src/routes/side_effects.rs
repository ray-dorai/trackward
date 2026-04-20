use axum::extract::{Path, State};
use axum::Json;
use chain_core::CanonicalField;
use uuid::Uuid;

use crate::actor::Actor;
use crate::chain::{compute_chain, ChainTable};
use crate::errors::Error;
use crate::models::{CreateSideEffect, SideEffect};
use crate::AppState;

pub async fn create(
    State(state): State<AppState>,
    actor: Actor,
    Json(input): Json<CreateSideEffect>,
) -> Result<Json<SideEffect>, Error> {
    let id = Uuid::now_v7();

    let mut tx = state.db.begin().await?;

    let fields = vec![
        CanonicalField::uuid("id", id),
        CanonicalField::uuid("run_id", input.run_id),
        CanonicalField::opt_uuid("tool_invocation_id", input.tool_invocation_id),
        CanonicalField::str("kind", &input.kind),
        CanonicalField::str("target", &input.target),
        CanonicalField::str("status", &input.status),
        CanonicalField::json("confirmation", input.confirmation.clone()),
        CanonicalField::timestamp("observed_at", input.observed_at),
        CanonicalField::str("actor_id", &actor.0),
    ];
    let link = compute_chain(&mut tx, ChainTable::SideEffects, input.run_id, &fields).await?;

    let row = sqlx::query_as::<_, SideEffect>(
        "INSERT INTO side_effects
            (id, run_id, tool_invocation_id, kind, target, status, confirmation, observed_at, actor_id, prev_hash, row_hash)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
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
    .bind(link.prev_hash.as_deref())
    .bind(&link.row_hash)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;

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
