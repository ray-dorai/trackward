# Security policy

trackward is an audit-trail product for regulated environments. A security
bug here is a direct compliance risk for our users. We treat reports
seriously and respond quickly.

## Reporting a vulnerability

**Do not open a public GitHub issue.** Send reports to
[security@trackward.dev](mailto:security@trackward.dev) (or, while that
mailbox is being provisioned, to the repository owner directly via the
email listed on their GitHub profile — include `[trackward-security]` in
the subject).

Include as much of the following as you can:

- A description of the issue and the impact you observed or inferred.
- Steps to reproduce, or a proof-of-concept.
- Affected version(s), commit hash if you've pinned one.
- Your name and affiliation, if you'd like credit.

If the issue involves secrets or signed bundles, please attach redacted
examples rather than live material.

## What to expect

| Step                         | Target time                         |
|------------------------------|-------------------------------------|
| Acknowledgement of report    | 2 business days                     |
| Initial triage + severity    | 5 business days                     |
| Fix or mitigation in a release | 30 days for high/critical, 90 days otherwise |
| Public advisory              | Coordinated with reporter after fix |

We'll keep you updated as triage progresses. If we decide not to act on a
report, we'll explain why.

## Scope

**In scope:**

- `services/ledger`, `services/gateway`, `tools/verifier`
- Deployment templates under `deploy/`
- CI workflows under `.github/workflows/`
- Cryptographic claims about the export-bundle format and row-level
  hash-chain (once that lands)

**Out of scope:**

- Vulnerabilities in third-party dependencies already disclosed upstream —
  please report those to the upstream project (we track them via
  `cargo deny check` in CI and will ship updates).
- Social engineering, physical attacks, DoS by resource exhaustion from
  a legitimately-authenticated client.
- Findings against a customer's own deployment that turn out to be
  misconfiguration (we'd still like to hear about it so we can improve the
  defaults or the docs).

## Safe harbor

If you act in good faith under this policy, we will:

- Not pursue or support legal action against you related to the report.
- Work with you to understand and resolve the issue.
- Credit you in the advisory if you want credit.

Good faith means: do not access or modify data you don't own, do not
degrade service availability, stop as soon as you have enough to write a
report, and give us a reasonable window to fix before disclosing publicly.

## Cryptographic claims

A few of trackward's guarantees are load-bearing — if any of the
following break, please report as critical:

- An export-bundle manifest verifies against its signature under the
  ledger's pinned public key, **and** the manifest hash matches the bytes
  shipped in the bundle.
- Once the row-level hash-chain lands (Phase 9), every row in a run
  chains unbroken to its predecessor in the same `(table, run_id)` group,
  and every such chain terminates at an anchored, signed merkle root.
- `actor_id` on every write row reflects the authenticated caller — it
  cannot be spoofed by an unauthenticated or differently-authenticated
  client.

## Not yet in place

In the interest of honesty rather than marketing: as of this writing we
do not have a bug bounty program, a formal PGP key for encrypted reports,
or a `security.txt` endpoint (we're pre-deployment). If you'd prefer to
send an encrypted report, ask for a key in your initial email.
