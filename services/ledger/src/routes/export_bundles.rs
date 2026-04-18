//! Signed export bundles.
//!
//! Building a bundle: load the case, load its linked evidence (sorted for
//! determinism), serialize a manifest, hash+sign the manifest bytes, and
//! persist the whole thing in `export_bundles`. The test
//! `signing_is_deterministic_for_same_manifest` locks in that two exports
//! with the same `fixed_signed_at` produce byte-identical manifests and
//! signatures — so the evidence order and every timestamp embedded in
//! the manifest must be stable, not wall-clock.

use axum::extract::{Path, State};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::Serialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::errors::Error;
use crate::models::{CaseEvidence, CreateExportBundle, ExportBundle};
use crate::AppState;

#[derive(Serialize)]
struct Manifest {
    case_id: Uuid,
    generated_at: DateTime<Utc>,
    evidence: Vec<ManifestEvidence>,
}

#[derive(Serialize)]
struct ManifestEvidence {
    #[serde(rename = "type")]
    kind: String,
    id: Uuid,
}

pub async fn create(
    State(state): State<AppState>,
    Path(case_id): Path<Uuid>,
    Json(input): Json<CreateExportBundle>,
) -> Result<Json<ExportBundle>, Error> {
    // Case must exist.
    let case_exists: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM cases WHERE id = $1")
        .bind(case_id)
        .fetch_optional(&state.db)
        .await?;
    if case_exists.is_none() {
        return Err(Error::NotFound);
    }

    let evidence = sqlx::query_as::<_, CaseEvidence>(
        "SELECT * FROM case_evidence WHERE case_id = $1
         ORDER BY evidence_type ASC, evidence_id ASC",
    )
    .bind(case_id)
    .fetch_all(&state.db)
    .await?;

    let signed_at = input.fixed_signed_at.unwrap_or_else(Utc::now);
    let manifest = Manifest {
        case_id,
        generated_at: signed_at,
        evidence: evidence
            .iter()
            .map(|e| ManifestEvidence {
                kind: e.evidence_type.clone(),
                id: e.evidence_id,
            })
            .collect(),
    };
    let manifest_json =
        serde_json::to_string(&manifest).expect("manifest serialization");
    let manifest_sha256 = hex::encode(Sha256::digest(manifest_json.as_bytes()));
    let signature = state.signing.sign_hex(manifest_json.as_bytes());

    let id = Uuid::now_v7();
    let row = sqlx::query_as::<_, ExportBundle>(
        "INSERT INTO export_bundles
            (id, case_id, manifest_json, manifest_sha256, signature,
             key_id, public_key_hex, signed_by, signed_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
         RETURNING *",
    )
    .bind(id)
    .bind(case_id)
    .bind(&manifest_json)
    .bind(&manifest_sha256)
    .bind(&signature)
    .bind(&state.signing.key_id)
    .bind(&state.signing.public_key_hex)
    .bind(&input.signed_by)
    .bind(signed_at)
    .fetch_one(&state.db)
    .await?;

    tracing::info!(
        id = %row.id,
        case_id = %row.case_id,
        key_id = %row.key_id,
        signed_by = %row.signed_by,
        "export_bundle signed"
    );
    Ok(Json(row))
}

pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ExportBundle>, Error> {
    let row = sqlx::query_as::<_, ExportBundle>("SELECT * FROM export_bundles WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(Error::NotFound)?;
    Ok(Json(row))
}
