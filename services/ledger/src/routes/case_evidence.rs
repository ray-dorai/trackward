use axum::extract::{Path, State};
use axum::Json;
use uuid::Uuid;

use crate::errors::Error;
use crate::models::{CaseEvidence, CreateCaseEvidence};
use crate::AppState;

pub async fn link(
    State(state): State<AppState>,
    Path(case_id): Path<Uuid>,
    Json(input): Json<CreateCaseEvidence>,
) -> Result<Json<CaseEvidence>, Error> {
    let row = sqlx::query_as::<_, CaseEvidence>(
        "INSERT INTO case_evidence
            (case_id, evidence_type, evidence_id, linked_by, linked_at, note)
         VALUES ($1, $2, $3, $4, $5, $6)
         RETURNING *",
    )
    .bind(case_id)
    .bind(&input.evidence_type)
    .bind(input.evidence_id)
    .bind(&input.linked_by)
    .bind(input.linked_at)
    .bind(input.note.as_deref())
    .fetch_one(&state.db)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(db_err) if db_err.is_unique_violation() => {
            Error::Conflict("evidence already linked to this case".into())
        }
        other => Error::Db(other),
    })?;

    tracing::info!(
        case_id = %row.case_id,
        evidence_type = %row.evidence_type,
        evidence_id = %row.evidence_id,
        linked_by = %row.linked_by,
        "case_evidence linked"
    );
    Ok(Json(row))
}

pub async fn list(
    State(state): State<AppState>,
    Path(case_id): Path<Uuid>,
) -> Result<Json<Vec<CaseEvidence>>, Error> {
    let rows = sqlx::query_as::<_, CaseEvidence>(
        "SELECT * FROM case_evidence WHERE case_id = $1 ORDER BY linked_at ASC",
    )
    .bind(case_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}
