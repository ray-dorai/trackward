use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;
use uuid::Uuid;

use crate::errors::Error;
use crate::models::{CreateCustodyEvent, CustodyEvent};
use crate::AppState;

pub async fn create(
    State(state): State<AppState>,
    Json(input): Json<CreateCustodyEvent>,
) -> Result<Json<CustodyEvent>, Error> {
    let id = Uuid::now_v7();
    let row = sqlx::query_as::<_, CustodyEvent>(
        "INSERT INTO custody_events
            (id, evidence_type, evidence_id, action, actor, reason,
             occurred_at, metadata)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         RETURNING *",
    )
    .bind(id)
    .bind(&input.evidence_type)
    .bind(input.evidence_id)
    .bind(&input.action)
    .bind(&input.actor)
    .bind(input.reason.as_deref())
    .bind(input.occurred_at)
    .bind(&input.metadata)
    .fetch_one(&state.db)
    .await?;

    tracing::info!(
        id = %row.id,
        evidence_type = %row.evidence_type,
        evidence_id = %row.evidence_id,
        action = %row.action,
        actor = %row.actor,
        "custody_event recorded"
    );
    Ok(Json(row))
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub evidence_type: String,
    pub evidence_id: Uuid,
}

pub async fn list(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<CustodyEvent>>, Error> {
    let rows = sqlx::query_as::<_, CustodyEvent>(
        "SELECT * FROM custody_events
         WHERE evidence_type = $1 AND evidence_id = $2
         ORDER BY occurred_at ASC",
    )
    .bind(&q.evidence_type)
    .bind(q.evidence_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}
