use axum::extract::{Path, State};
use axum::Json;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::actor::Actor;
use crate::errors::Error;
use crate::models::{Case, CaseEvidence, CreateCase};
use crate::AppState;

pub async fn create(
    State(state): State<AppState>,
    actor: Actor,
    Json(input): Json<CreateCase>,
) -> Result<Json<Case>, Error> {
    let id = Uuid::now_v7();
    let row = sqlx::query_as::<_, Case>(
        "INSERT INTO cases
            (id, title, description, opened_by, opened_at, metadata, actor_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         RETURNING *",
    )
    .bind(id)
    .bind(&input.title)
    .bind(&input.description)
    .bind(&input.opened_by)
    .bind(input.opened_at)
    .bind(&input.metadata)
    .bind(&actor.0)
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

/// Resolve every linked piece of evidence in one payload. Unknown
/// evidence_types come back with `resolved: null` rather than 500ing —
/// so adding a new evidence_type somewhere doesn't break existing
/// dossier consumers.
pub async fn dossier(
    State(state): State<AppState>,
    Path(case_id): Path<Uuid>,
) -> Result<Json<Value>, Error> {
    let case = sqlx::query_as::<_, Case>("SELECT * FROM cases WHERE id = $1")
        .bind(case_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(Error::NotFound)?;

    let links = sqlx::query_as::<_, CaseEvidence>(
        "SELECT * FROM case_evidence WHERE case_id = $1 ORDER BY linked_at ASC",
    )
    .bind(case_id)
    .fetch_all(&state.db)
    .await?;

    let mut entries = Vec::with_capacity(links.len());
    for link in links {
        let resolved = resolve_evidence(&state, &link.evidence_type, link.evidence_id).await?;
        entries.push(json!({ "link": link, "resolved": resolved }));
    }

    Ok(Json(json!({ "case": case, "evidence": entries })))
}

async fn resolve_evidence(
    state: &AppState,
    evidence_type: &str,
    evidence_id: Uuid,
) -> Result<Option<Value>, Error> {
    match evidence_type {
        "run" => {
            let row: Option<crate::models::Run> =
                sqlx::query_as::<_, crate::models::Run>("SELECT * FROM runs WHERE id = $1")
                    .bind(evidence_id)
                    .fetch_optional(&state.db)
                    .await?;
            Ok(row.map(|r| serde_json::to_value(r).unwrap()))
        }
        "artifact" => {
            let row: Option<crate::models::Artifact> =
                sqlx::query_as::<_, crate::models::Artifact>(
                    "SELECT * FROM artifacts WHERE id = $1",
                )
                .bind(evidence_id)
                .fetch_optional(&state.db)
                .await?;
            Ok(row.map(|r| serde_json::to_value(r).unwrap()))
        }
        "tool_invocation" => {
            let row: Option<crate::models::ToolInvocation> =
                sqlx::query_as::<_, crate::models::ToolInvocation>(
                    "SELECT * FROM tool_invocations WHERE id = $1",
                )
                .bind(evidence_id)
                .fetch_optional(&state.db)
                .await?;
            Ok(row.map(|r| serde_json::to_value(r).unwrap()))
        }
        _ => Ok(None),
    }
}
