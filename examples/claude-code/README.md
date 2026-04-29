# Claude Code → Trackward

Six bash hooks that wire any Claude Code install into Trackward, producing
a hash-chained, signed audit dossier for every session.

## What gets recorded

- **`SessionStart`** — mints a Trackward run, stashes its id at
  `/tmp/trackward-claude-<session_id>.json` for the rest of the hooks.
- **`UserPromptSubmit`** — every user prompt as a `user_message` event.
- **`PreToolUse`** — every tool call (Bash, Read, Write, Edit, …) as a
  `tool_call` event with a synthesized `call_id`. **Synchronous and
  blocking** by Claude Code's contract: an exit code of 2 here would
  refuse the tool. (Today these hooks always allow; policy enforcement
  is a separate concern.)
- **`PostToolUse`** — paired `tool_result` event plus a first-class
  `tool_invocation` row, correlated by `call_id`.
- **`Stop`** — extracts the final assistant message from the transcript
  and records it as `model_response`, then a `task_complete` marker.
- **`SessionEnd`** — opens a case for the run, links the run as
  evidence, exports a signed dossier bundle to
  `$TRACKWARD_BUNDLE_DIR` (default `/tmp`).

## Why hooks (not a passive log scrape)

The hook fires *before* the tool runs and gets the tool input. If
hooks are configured, every tool call goes through them — bypassing
requires editing `settings.json`, which is itself part of the
filesystem the customer audits. That gives the strong guarantee
Trackward's design assumes ("if it didn't go through the recorder, it
didn't happen") rather than the best-effort guarantee of scraping
session logs after the fact.

## Install

1. Bring up the Trackward stack (`docker compose up -d` + run ledger
   and gateway, or `helm install` against your cluster).

2. Copy `settings.json` into `~/.claude/settings.json` (user-scope) or
   `.claude/settings.json` in your repo (project-scope). Replace
   `/PATH/TO/trackward` with the absolute path to your checkout. You
   can merge with an existing `hooks` block — Claude Code runs all
   matching hooks in order.

3. Configure env (in your shell profile or `~/.claude/env`):

   ```sh
   export TRACKWARD_LEDGER_URL=http://localhost:3000
   export TRACKWARD_ACTOR=claude-code@your-name
   # Optional. Required when LEDGER_AUTH_TOKEN is set on the ledger.
   export TRACKWARD_LEDGER_TOKEN=...
   # Optional. Where bundles get dropped on SessionEnd.
   export TRACKWARD_BUNDLE_DIR=$HOME/trackward-bundles
   ```

4. Start a fresh Claude Code session. At session end, the hook prints
   the bundle path. Verify it offline:

   ```sh
   verifier $TRACKWARD_BUNDLE_DIR/trackward-claude-<session_id>.bundle.json
   ```

## What this proves to a customer

Every Bash command, every file read, every file edit, every model turn
in a Claude Code session lands in an append-only Postgres table,
chained to its predecessor by a hash that includes the actor, anchored
periodically to a signed merkle root in S3 WORM, and exportable as a
self-contained bundle the auditor verifies offline with no callback to
your infrastructure. Drop a few hooks into `settings.json`, get
audit-grade evidence of agent behaviour.

## Limits worth naming

- **Session-state file in `/tmp`** is not durable across machine
  restarts mid-session. Acceptable for a v1; revisit if customers run
  Claude Code in long-lived containers that survive ledger blips.
- **`call_id` is synthesized client-side** (timestamp + pid). Good
  enough for single-machine sessions; not unique across a fleet.
- **Hooks run under the user's shell environment.** If a hook script
  fails (network down, ledger unreachable), Claude Code by default
  continues — meaning there *can* be silent gaps. Set
  `failOnHookError: true` in `settings.json` (Claude Code option) if
  you want hard-stop behaviour. Trackward's preferred posture:
  fail-loud, since a missing hook is the audit story breaking.
