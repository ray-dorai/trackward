use axum::extract::{Path, State};
use axum::Json;
use chain_core::CanonicalField;
use uuid::Uuid;

use crate::actor::Actor;
use crate::chain::{compute_chain, ChainTable};
use crate::errors::Error;
use crate::models::{CreateEvent, Event};
use crate::AppState;

pub async fn append(
    State(state): State<AppState>,
    actor: Actor,
    Path(run_id): Path<Uuid>,
    Json(input): Json<CreateEvent>,
) -> Result<Json<Event>, Error> {
    // Serialize concurrent appends to the same run. This lock is held at
    // transaction scope, so committing or rolling back releases it. It
    // also gates the Phase 9a chain lookup — `compute_chain` takes a
    // separate chain-specific advisory lock, but this one still matters
    // because the `MAX(seq)+1` read below must not race with another
    // appender on the same run.
    let mut tx = state.db.begin().await?;

    let lock_key = uuid_to_lock_key(run_id);
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(lock_key)
        .execute(&mut *tx)
        .await?;

    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM runs WHERE id = $1)")
        .bind(run_id)
        .fetch_one(&mut *tx)
        .await?;
    if !exists {
        return Err(Error::NotFound);
    }

    let id = Uuid::now_v7();

    // Assign seq first so it participates in the chain hash — a DBA who
    // silently rewrote `seq` would break the chain downstream.
    let seq: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM events WHERE run_id = $1",
    )
    .bind(run_id)
    .fetch_one(&mut *tx)
    .await?;

    let fields = vec![
        CanonicalField::uuid("id", id),
        CanonicalField::uuid("run_id", run_id),
        CanonicalField::i64("seq", seq),
        CanonicalField::str("kind", &input.kind),
        CanonicalField::json("body", input.body.clone()),
        CanonicalField::timestamp("occurred_at", input.occurred_at),
        CanonicalField::str("actor_id", &actor.0),
    ];
    let link = compute_chain(&mut tx, ChainTable::Events, run_id, &fields).await?;

    let event = sqlx::query_as::<_, Event>(
        "INSERT INTO events
            (id, run_id, seq, kind, body, occurred_at, actor_id, prev_hash, row_hash)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
         RETURNING *",
    )
    .bind(id)
    .bind(run_id)
    .bind(seq)
    .bind(&input.kind)
    .bind(&input.body)
    .bind(input.occurred_at)
    .bind(&actor.0)
    .bind(link.prev_hash.as_deref())
    .bind(&link.row_hash)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;

    tracing::info!(
        event_id = %event.id,
        run_id = %run_id,
        seq = event.seq,
        kind = %event.kind,
        actor_id = %event.actor_id,
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
