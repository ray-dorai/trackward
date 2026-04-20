//! Read-only routes for merkle anchors.
//!
//! There is no write route — anchors are always produced by the
//! background task (or, in tests, by calling
//! `crate::anchoring::anchor_tick` directly). Exposing `POST /anchors`
//! would let an untrusted caller pin a root over a window of their
//! choosing, which is the opposite of what anchors are for.

use axum::extract::{Path, Query, State};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use uuid::Uuid;

use crate::anchoring::{anchor_tick, AnchorScope};
use crate::errors::Error;
use crate::models::Anchor;
use crate::AppState;

pub async fn get(State(state): State<AppState>, Path(id): Path<Uuid>) -> Result<Json<Anchor>, Error> {
    let row = sqlx::query_as::<_, Anchor>("SELECT * FROM anchors WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(Error::NotFound)?;
    Ok(Json(row))
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub run_id: Option<Uuid>,
    #[serde(default)]
    pub global: bool,
}

/// List anchors for a scope, newest first. `?global=true` for global
/// scope, `?run_id=<uuid>` for a specific run. Without either, returns
/// every anchor in the table (smallest useful surface for Phase 9b).
pub async fn list(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<Anchor>>, Error> {
    let rows: Vec<Anchor> = if q.global {
        sqlx::query_as::<_, Anchor>(
            "SELECT * FROM anchors WHERE run_id IS NULL ORDER BY anchored_to DESC",
        )
        .fetch_all(&state.db)
        .await?
    } else if let Some(run_id) = q.run_id {
        sqlx::query_as::<_, Anchor>(
            "SELECT * FROM anchors WHERE run_id = $1 ORDER BY anchored_to DESC",
        )
        .bind(run_id)
        .fetch_all(&state.db)
        .await?
    } else {
        sqlx::query_as::<_, Anchor>("SELECT * FROM anchors ORDER BY anchored_to DESC")
            .fetch_all(&state.db)
            .await?
    };
    Ok(Json(rows))
}

#[derive(Debug, Deserialize)]
pub struct TriggerBody {
    pub run_id: Option<Uuid>,
    #[serde(default)]
    pub global: bool,
    /// Upper bound of the anchor window. Defaults to `now()`.
    pub until: Option<DateTime<Utc>>,
}

/// Test-mode and on-demand endpoint: trigger an anchor tick
/// synchronously. Normally anchoring is the background loop's job;
/// this route lets integration tests drive anchoring deterministically
/// and gives operators a manual lever for drills. Writes nothing
/// unless the window has leaves (returns 204-equivalent empty body).
pub async fn trigger(
    State(state): State<AppState>,
    Json(body): Json<TriggerBody>,
) -> Result<Json<Option<Anchor>>, Error> {
    let scope = match (body.global, body.run_id) {
        (true, None) => AnchorScope::Global,
        (false, Some(id)) => AnchorScope::Run(id),
        (true, Some(_)) => {
            return Err(Error::BadRequest(
                "cannot specify both global and run_id".into(),
            ))
        }
        (false, None) => {
            return Err(Error::BadRequest(
                "must specify either global or run_id".into(),
            ))
        }
    };
    let until = body.until.unwrap_or_else(Utc::now);
    let anchor = anchor_tick(&state.db, &state.signing, &state.anchor_sink, scope, until).await?;
    Ok(Json(anchor))
}
