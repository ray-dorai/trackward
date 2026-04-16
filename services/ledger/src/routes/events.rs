use axum::extract::{Path, State};
use axum::Json;
use uuid::Uuid;

use crate::errors::Error;
use crate::models::{CreateEvent, Event};
use crate::AppState;

pub async fn append(
    State(state): State<AppState>,
    Path(run_id): Path<Uuid>,
    Json(input): Json<CreateEvent>,
) -> Result<Json<Event>, Error> {
    // Use a transaction with an advisory lock keyed on run_id so concurrent
    // appends to the same run are serialized. Reads across runs are unaffected.
    let mut tx = state.db.begin().await?;

    // pg_advisory_xact_lock takes a bigint; hash the UUID to a u64.
    let lock_key = uuid_to_lock_key(run_id);
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(lock_key)
        .execute(&mut *tx)
        .await?;

    // Confirm the run exists — fail cleanly rather than orphan an event
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM runs WHERE id = $1)")
        .bind(run_id)
        .fetch_one(&mut *tx)
        .await?;
    if !exists {
        return Err(Error::NotFound);
    }

    let id = Uuid::now_v7();
    let event = sqlx::query_as::<_, Event>(
        "INSERT INTO events (id, run_id, seq, kind, body, occurred_at)
         SELECT $1, $2, COALESCE(MAX(seq), 0) + 1, $3, $4, $5 FROM events WHERE run_id = $2
         RETURNING *",
    )
    .bind(id)
    .bind(run_id)
    .bind(&input.kind)
    .bind(&input.body)
    .bind(input.occurred_at)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;

    tracing::info!(
        event_id = %event.id,
        run_id = %run_id,
        seq = event.seq,
        kind = %event.kind,
        "event appended"
    );
    Ok(Json(event))
}

pub async fn list(
    State(state): State<AppState>,
    Path(run_id): Path<Uuid>,
) -> Result<Json<Vec<Event>>, Error> {
    let events = sqlx::query_as::<_, Event>(
        "SELECT * FROM events WHERE run_id = $1 ORDER BY seq ASC",
    )
    .bind(run_id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(events))
}

/// Convert a UUID into a stable i64 lock key for pg_advisory_xact_lock.
fn uuid_to_lock_key(id: Uuid) -> i64 {
    let bytes = id.as_bytes();
    let mut key: u64 = 0;
    for (i, &b) in bytes.iter().enumerate() {
        key ^= (b as u64) << ((i % 8) * 8);
    }
    key as i64
}
