# Runbook: key rotation

**Status:** v1 — designed but not yet drilled. Plan to exercise against
`docker-compose` first, then against a staging cluster, before customer
production use.

## Keys in scope

Three classes of secret material this stack relies on:

1. **Ledger signing key** (`LEDGER_SIGNING_KEY_HEX`) — 32-byte ed25519
   seed. Signs anchor manifests and export bundles. The most
   load-bearing key in the system; rotation is the trickiest.
2. **mTLS certificates** (`tls.crt`, `tls.key`, `ca.crt` in
   `trackward-ledger` and `trackward-gateway` secrets) — when
   `mtls.enabled=true`. Issued from the customer's own CA.
3. **Postgres + S3 credentials** (`trackward-postgres`,
   `trackward-s3`). Standard cloud-credential rotation; not specific
   to this product.

## Ledger signing key rotation

### What happens at sign time

`SigningService::from_env` reads the seed once at process start. Every
anchor and export bundle stamps `key_id = sha256(public_key)` and
embeds the full public key. A verifier never needs to call back to the
ledger to know which key signed what.

### Rotation procedure

1. **Generate a new seed.** 32 random bytes, hex-encoded.

   ```sh
   head -c 32 /dev/urandom | xxd -p -c 32
   ```

2. **Update the secret with the new seed.** Keep the old key in your
   secret store (KMS, Vault, Sealed Secrets — whatever the operator
   uses) under an archival name so verifiers can still validate
   pre-rotation anchors.

3. **Rolling restart of the ledger.** New anchors will be signed with
   the new key from this point forward. Existing anchors retain their
   original `key_id` and remain verifiable forever.

4. **Distribute the new public key** to anyone who runs the verifier
   offline. They need the *set* of public keys (old + new) to
   validate bundles spanning the rotation window. Public-key
   distribution channel is the operator's choice — typically a
   well-known JSON document hosted at a stable URL.

### What does *not* require rotation

Existing anchors and bundles are immutable. They were signed by the
old key, and the bundle/anchor JSON carries the public key of its
signer. A verifier given both old and new public keys validates
pre- and post-rotation artifacts identically.

### What breaks if you do this wrong

- **Lose the old seed before all old anchors are out of legal hold:**
  you can no longer prove those anchors were yours. They still
  validate against the public key, but you cannot demonstrate
  possession.
- **Reuse a seed across deployments:** anchors signed by both deploys
  carry the same `key_id`, making it impossible to attribute anchors
  to a specific environment. Use a fresh seed per deployment.

## mTLS rotation

Standard X.509 rotation. Renew the cert from the customer CA before
expiry; update the secret named in `ledger.existingSecret` /
`gateway.existingSecret`; rolling restart picks up the new cert.

The chart's `mtls.enabled=true` posture means a missing or expired
cert refuses to start, which is the desired failure mode (loud, at
boot — see `services/ledger/src/tls.rs`).

## Postgres / S3 credentials

Update the secret values; rolling restart. The chart re-reads them on
pod start. There is no in-flight connection draining for credential
rotation; expect a brief 5xx blip on each pod restart.

## Verify the rotation

After any rotation, run `scripts/drill-restore.sh` to confirm the
end-to-end path still produces a verifier-OK bundle.
