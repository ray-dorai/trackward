# Claude Code → Trackward

Six bash hooks that wire any Claude Code install into Trackward, producing
a hash-chained, signed audit dossier for every session.

## What gets recorded

Two parallel streams cover the same activity from different angles:

**Live event stream** (fired by hooks at the moment things happen):

- **`SessionStart`** — mints a Trackward run, stashes its id at
  `/tmp/trackward-claude-<session_id>.json`.
- **`UserPromptSubmit`** — user prompt as `user_message` event.
- **`PreToolUse`** — every tool call as `tool_call` with a synthesized
  `call_id`. Synchronous and blocking by Claude Code's contract: exit
  code 2 would refuse the tool. (Today these always allow.)
- **`PostToolUse`** — paired `tool_result` event plus a first-class
  `tool_invocation` row.
- **`Stop`** — terminal assistant message as `model_response`, then
  `task_complete` marker. Fires only when the agent truly stops (no
  more iterations queued), so it's sparse.
- **`SessionEnd`** — opens a case for the run, links the run as
  evidence, exports a signed dossier bundle to
  `$TRACKWARD_BUNDLE_DIR` (default `/tmp`), uploads the raw
  transcript file as a `claude-code-transcript` artifact.

**Transcript snapshot stream** (`trackward-snapshot.sh` fired alongside
UserPromptSubmit, PostToolUse, Stop, SessionEnd) — POSTs every line
of Claude Code's transcript JSONL that hasn't been posted yet, as a
typed `transcript_*` event:

- `transcript_assistant_text`, `transcript_assistant_thinking`,
  `transcript_assistant_tool_use` (one per content block in an
  assistant turn)
- `transcript_user` (user prompts AND tool results, since Claude Code
  stores both under role=user)
- `transcript_system`, `transcript_attachment`,
  `transcript_permission_mode`, `transcript_queue_operation`,
  `transcript_file_history_snapshot`, `transcript_last_prompt`

A per-session cursor at `/tmp/trackward-claude-<sid>-cursor` makes the
snapshot idempotent — every transcript line is posted exactly once.

**Why both streams.** The live events are the realtime audit signal
(every tool call recorded synchronously, before it runs, hash-chained
in order). The transcript stream is the completeness guarantee
(literally every line Claude Code writes to disk lands in the dossier).
They overlap on tool calls; the dossier reader can pick whichever they
need.

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

- **Crash window between snapshots.** The snapshot fires on
  UserPromptSubmit / PostToolUse / Stop / SessionEnd. A `kill -9` (or
  ledger-unreachable error) between those events loses transcript
  lines written in that interval. Live tool calls are always safe
  (PreToolUse/PostToolUse fired synchronously). Ground-truth recovery:
  the SessionEnd transcript-artifact upload — if even that's missed,
  the operator still has the original transcript file on disk and can
  re-mirror with `trackward-snapshot.sh` against the existing run_id.
- **Session-state files in `/tmp`** are not durable across machine
  restarts mid-session. Acceptable for v1; revisit if customers run
  Claude Code in long-lived containers that survive a host reboot.
- **`call_id` is synthesized client-side** (timestamp + pid). Good
  enough for single-machine sessions; not unique across a fleet.
- **Hooks run under the user's shell environment.** If a hook script
  fails (network down, ledger unreachable), Claude Code by default
  continues — meaning there *can* be silent gaps. Set
  `failOnHookError: true` in `settings.json` if you want hard-stop
  behaviour. Trackward's preferred posture: fail-loud, since a missing
  hook is the audit story breaking.
