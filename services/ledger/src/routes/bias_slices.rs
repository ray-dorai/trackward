use axum::extract::{Path, State};
use axum::Json;
use chain_core::CanonicalField;
use uuid::Uuid;

use crate::actor::Actor;
use crate::chain::{compute_chain, ChainTable};
use crate::errors::Error;
use crate::models::{BiasSlice, CreateBiasSlice};
use crate::AppState;

pub async fn create(
    State(state): State<AppState>,
    actor: Actor,
    Json(input): Json<CreateBiasSlice>,
) -> Result<Json<BiasSlice>, Error> {
    // The CHECK constraint catches (NULL, NULL) at the DB layer — reject it
    // up front with a cleaner 400 so callers aren't chasing a 500.
    if input.run_id.is_none() && input.eval_result_id.is_none() {
        return Err(Error::BadRequest(
            "bias_slice requires either run_id or eval_result_id".into(),
        ));
    }

    let id = Uuid::now_v7();

    let mut tx = state.db.begin().await?;

    // Chain only when this bias_slice belongs to a run. Eval-time slices
    // (no run_id) get `prev_hash = NULL, row_hash = zeros` — the legacy
    // marker. The chain covers the run-scoped slices, which is the
    // audit-relevant case; eval-registry slices are immutable by
    // content-hash via the registry's own machinery.
    let (prev_hash, row_hash): (Option<Vec<u8>>, Vec<u8>) = match input.run_id {
        Some(run_id) => {
            // Score is f64 which canonical_json deliberately rejects
            // (lossy across languages). Serialize using Rust's shortest
            // round-trip string form; that's stable for finite floats.
            // Non-finite floats shouldn't appear in the schema; we reject
            // them here rather than hash a platform-dependent "NaN".
            let score_str = input.score.map(|f| {
                assert!(f.is_finite(), "bias_slice.score must be finite");
                f.to_string()
            });
            let fields = vec![
                CanonicalField::uuid("id", id),
                CanonicalField::uuid("run_id", run_id),
                CanonicalField::opt_uuid("eval_result_id", input.eval_result_id),
                CanonicalField::str("label", &input.label),
                CanonicalField::opt_str("value", input.value.clone()),
                CanonicalField::opt_str("score", score_str),
                CanonicalField::json("metadata", input.metadata.clone()),
                CanonicalField::str("actor_id", &actor.0),
            ];
            let link = compute_chain(&mut tx, ChainTable::BiasSlices, run_id, &fields).await?;
            (link.prev_hash, link.row_hash)
        }
        None => (None, crate::chain::LEGACY_ROW_HASH.to_vec()),
    };

    let row = sqlx::query_as::<_, BiasSlice>(
        "INSERT INTO bias_slices
            (id, run_id, eval_result_id, label, value, score, metadata, actor_id, prev_hash, row_hash)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         RETURNING *",
    )
    .bind(id)
    .bind(input.run_id)
    .bind(input.eval_result_id)
    .bind(&input.label)
    .bind(input.value.as_deref())
    .bind(input.score)
    .bind(&input.metadata)
    .bind(&actor.0)
    .bind(prev_hash.as_deref())
    .bind(&row_hash)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;

    tracing::info!(
        id = %row.id,
        run_id = ?row.run_id,
        eval_result_id = ?row.eval_result_id,
        label = %row.label,
        "bias_slice recorded"
    );
    Ok(Json(row))
}

pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<BiasSlice>, Error> {
    let row = sqlx::query_as::<_, BiasSlice>("SELECT * FROM bias_slices WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(Error::NotFound)?;
    Ok(Json(row))
}

pub async fn list_for_run(
    State(state): State<AppState>,
    Path(run_id): Path<Uuid>,
) -> Result<Json<Vec<BiasSlice>>, Error> {
    let rows = sqlx::query_as::<_, BiasSlice>(
        "SELECT * FROM bias_slices WHERE run_id = $1 ORDER BY created_at ASC",
    )
    .bind(run_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}
