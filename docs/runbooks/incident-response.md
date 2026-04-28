# Runbook: incident response

**Status:** v1 — process designed; not yet exercised in a real incident
or tabletop. Update after the first drill.

## Scope

This runbook covers operational incidents on a Trackward deployment:
data-integrity events, suspected unauthorized writes, key compromise,
and availability incidents that affect the audit trail.

For *vulnerability disclosures from external researchers*, see
`SECURITY.md`. For *customer support*, that is a separate process
your support function owns.

## Severity

| | Definition | Examples |
|---|---|---|
| **P0** | Audit story broken or compromised | verifier reports `FAIL` on a customer bundle; chain forked; signing key suspected compromised; signed anchor doesn't match rebuilt root |
| **P1** | Integrity intact, but evidence of unauthorized access or near-miss | unauthorized read against ledger; mTLS bypass attempted; secret leaked but no write observed |
| **P2** | Availability or operational degradation, no integrity impact | gateway/ledger down; anchor loop stalled; restore drill failed |

## On detection

### 1. Capture state before touching anything

The single most important thing in the first ten minutes is **don't
destroy evidence**. The ledger is your evidence. Do not restart pods,
do not roll back, do not rotate keys yet.

```sh
# Snapshot: ledger logs (last hour), anchor loop logs, recent rows.
kubectl logs -n trackward -l app=ledger --since=1h > inc-$(date -u +%Y%m%dT%H%M%SZ)-ledger.log
kubectl logs -n trackward -l app=gateway --since=1h > inc-$(date -u +%Y%m%dT%H%M%SZ)-gateway.log
pg_dump --format=custom --dbname="$DATABASE_URL" --file=inc-$(date -u +%Y%m%dT%H%M%SZ).pgdump
aws s3 ls "s3://$ANCHOR_BUCKET/" --recursive > inc-anchors.txt
```

These artifacts go into a single incident bundle that lives outside
the affected cluster.

### 2. Triage

Run the verifier against the most recent customer-facing bundle and
against the most recent anchor:

```sh
verifier latest-bundle.json
verifier --anchor latest-anchor.json leaves.txt
```

The verifier output disambiguates:
- `OK` on bundle + `OK` on anchor → integrity intact; treat as P1 or P2
- `FAIL` on bundle but `OK` on anchor → row(s) tampered post-anchor;
  the anchor is your authoritative reference. Re-derive what the
  bundle *should* contain from un-tampered rows.
- `FAIL` on anchor → either the anchor was forged or the chain was
  rebuilt under a rogue key. P0; escalate.

### 3. Contain (P0/P1 only)

- **Suspected key compromise:** stop the ledger (`kubectl scale deploy
  ledger --replicas=0`). New writes cannot be signed-and-anchored
  while the ledger is down, so the chain stops growing rather than
  growing under a compromised key.
- **Suspected ongoing tamper:** the same. The ledger being down is
  better than the ledger writing more rows you can't trust.
- **Network breach scoped to the cluster:** verify the
  `networkPolicy.enabled=true` allowlist is intact (nothing should
  egress except Postgres and the two S3 buckets).

### 4. Recover

Per `backup-restore.md`. Restore from the last verifier-OK snapshot.
Document the gap between backup time and incident detection — that
window is what customers will ask about.

If keys were compromised, follow `key-rotation.md` and treat the
compromised key as archival (cannot validate post-rotation anchors,
but old anchors remain valid because their public key is embedded).

### 5. Notify

- **Affected customers:** within the SLA committed in your contract.
  At minimum, what happened, what's affected, when service is
  restored, what they need to do.
- **Internal record:** write the incident itself to the ledger as a
  custody event so the audit trail of the audit-trail product is
  itself auditable.

### 6. Postmortem

Within 5 business days. Sections: timeline, root cause, what worked,
what didn't, follow-up actions with owners and dates. Specifically
ask: *did this incident expose a hole in another runbook?* If yes,
update that runbook before closing the postmortem.

## Things that should never happen, and what they mean if they do

- **A successful UPDATE or DELETE against a ledger table.** The
  schema disallows these via triggers (`services/ledger/AGENTS.md`).
  If one succeeds, the migration is corrupt or someone has direct
  superuser access to Postgres. P0.
- **Two anchors with overlapping `(anchored_from, anchored_to)`
  ranges.** The anchoring loop is restart-safe; overlap means two
  ledger processes ran against the same DB simultaneously. Stop one,
  preserve both anchors, write a custody event.
- **A missing actor_id on a write-path row.** Migration 0009 made
  this column `not null`. A row missing it means the column was
  rewritten directly. P0.
