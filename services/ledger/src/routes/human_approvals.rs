use axum::extract::{Path, State};
use axum::Json;
use chain_core::CanonicalField;
use uuid::Uuid;

use crate::actor::Actor;
use crate::chain::{compute_chain, ChainTable};
use crate::errors::Error;
use crate::models::{CreateHumanApproval, HumanApproval};
use crate::AppState;

pub async fn create(
    State(state): State<AppState>,
    actor: Actor,
    Json(input): Json<CreateHumanApproval>,
) -> Result<Json<HumanApproval>, Error> {
    let mut tx = state.db.begin().await?;

    let fields = vec![
        CanonicalField::uuid("id", input.id),
        CanonicalField::uuid("run_id", input.run_id),
        CanonicalField::str("tool", &input.tool),
        CanonicalField::str("decision", &input.decision),
        CanonicalField::opt_str("reason", input.reason.clone()),
        CanonicalField::opt_str("decided_by", input.decided_by.clone()),
        CanonicalField::timestamp("requested_at", input.requested_at),
        CanonicalField::timestamp("decided_at", input.decided_at),
        CanonicalField::json("metadata", input.metadata.clone()),
        CanonicalField::str("actor_id", &actor.0),
    ];
    let link = compute_chain(&mut tx, ChainTable::HumanApprovals, input.run_id, &fields).await?;

    let row = sqlx::query_as::<_, HumanApproval>(
        "INSERT INTO human_approvals
            (id, run_id, tool, decision, reason, decided_by,
             requested_at, decided_at, metadata, actor_id, prev_hash, row_hash)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
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
    .bind(&actor.0)
    .bind(link.prev_hash.as_deref())
    .bind(&link.row_hash)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| match e {
        // id collision (approval already recorded) — keep it a conflict
        // rather than a 500.
        sqlx::Error::Database(db_err) if db_err.is_unique_violation() => {
            Error::Conflict("human_approval already recorded".into())
        }
        other => Error::Db(other),
    })?;

    tx.commit().await?;

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
