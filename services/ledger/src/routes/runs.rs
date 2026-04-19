use axum::extract::{Path, Query, State};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::actor::Actor;
use crate::errors::Error;
use crate::models::{CreateRun, Run};
use crate::AppState;

pub async fn create(
    State(state): State<AppState>,
    actor: Actor,
    Json(input): Json<CreateRun>,
) -> Result<Json<Run>, Error> {
    let id = Uuid::now_v7();
    let run = sqlx::query_as::<_, Run>(
        "INSERT INTO runs (id, agent, started_at, metadata, actor_id)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING *",
    )
    .bind(id)
    .bind(&input.agent)
    .bind(input.started_at)
    .bind(&input.metadata)
    .bind(&actor.0)
    .fetch_one(&state.db)
    .await?;

    tracing::info!(run_id = %run.id, agent = %run.agent, actor_id = %run.actor_id, "run created");
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

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub agent: Option<String>,
    pub since: Option<DateTime<Utc>>,
    pub until: Option<DateTime<Utc>>,
    pub limit: Option<i64>,
}

pub async fn list(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<Run>>, Error> {
    // Build a parameterized query. Every filter is optional; when unset
    // the corresponding predicate is true for every row. limit defaults
    // to 100.
    let limit = q.limit.unwrap_or(100).clamp(1, 1000);
    let runs = sqlx::query_as::<_, Run>(
        "SELECT * FROM runs
         WHERE ($1::text IS NULL OR agent = $1)
           AND ($2::timestamptz IS NULL OR started_at >= $2)
           AND ($3::timestamptz IS NULL OR started_at < $3)
         ORDER BY started_at DESC
         LIMIT $4",
    )
    .bind(q.agent.as_deref())
    .bind(q.since)
    .bind(q.until)
    .bind(limit)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(runs))
}

/// Everything the ledger has for a single run, in one shot. The haruspex
/// uses this as the primary "tell me what this run did" endpoint.
pub async fn dossier(
    State(state): State<AppState>,
    Path(run_id): Path<Uuid>,
) -> Result<Json<Value>, Error> {
    let run: Option<Run> = sqlx::query_as::<_, Run>("SELECT * FROM runs WHERE id = $1")
        .bind(run_id)
        .fetch_optional(&state.db)
        .await?;
    let run = run.ok_or(Error::NotFound)?;

    let events = sqlx::query_as::<_, crate::models::Event>(
        "SELECT * FROM events WHERE run_id = $1 ORDER BY seq ASC",
    )
    .bind(run_id)
    .fetch_all(&state.db)
    .await?;

    let tool_invocations = sqlx::query_as::<_, crate::models::ToolInvocation>(
        "SELECT * FROM tool_invocations WHERE run_id = $1 ORDER BY started_at ASC",
    )
    .bind(run_id)
    .fetch_all(&state.db)
    .await?;

    let side_effects = sqlx::query_as::<_, crate::models::SideEffect>(
        "SELECT * FROM side_effects WHERE run_id = $1 ORDER BY observed_at ASC",
    )
    .bind(run_id)
    .fetch_all(&state.db)
    .await?;

    let guardrails = sqlx::query_as::<_, crate::models::Guardrail>(
        "SELECT * FROM guardrails WHERE run_id = $1 ORDER BY evaluated_at ASC",
    )
    .bind(run_id)
    .fetch_all(&state.db)
    .await?;

    let human_approvals = sqlx::query_as::<_, crate::models::HumanApproval>(
        "SELECT * FROM human_approvals WHERE run_id = $1 ORDER BY decided_at ASC",
    )
    .bind(run_id)
    .fetch_all(&state.db)
    .await?;

    let bias_slices = sqlx::query_as::<_, crate::models::BiasSlice>(
        "SELECT * FROM bias_slices WHERE run_id = $1 ORDER BY created_at ASC",
    )
    .bind(run_id)
    .fetch_all(&state.db)
    .await?;

    let artifacts = sqlx::query_as::<_, crate::models::Artifact>(
        "SELECT * FROM artifacts WHERE run_id = $1 ORDER BY created_at ASC",
    )
    .bind(run_id)
    .fetch_all(&state.db)
    .await?;

    let binding: Option<crate::models::RunVersionBinding> =
        sqlx::query_as::<_, crate::models::RunVersionBinding>(
            "SELECT * FROM run_version_bindings WHERE run_id = $1",
        )
        .bind(run_id)
        .fetch_optional(&state.db)
        .await?;

    Ok(Json(json!({
        "run": run,
        "events": events,
        "tool_invocations": tool_invocations,
        "side_effects": side_effects,
        "guardrails": guardrails,
        "human_approvals": human_approvals,
        "bias_slices": bias_slices,
        "artifacts": artifacts,
        "binding": binding,
    })))
}
