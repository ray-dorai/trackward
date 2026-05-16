#!/usr/bin/env python3
"""
trackward-model-proxy — Tier 1 capture.

HTTP proxy between an agent and a model API (Anthropic, OpenAI, or any
similarly-shaped JSON API). Mirrors every request and response into the
Trackward ledger as `model_request` / `model_response` events, then
forwards transparently to the upstream.

Captures the agent's *intent* before it acts. Universal because every
agent talks to a model API.

Usage:
    trackward-model-proxy --upstream https://api.anthropic.com [--port 8088]

Then point the agent at it:
    export ANTHROPIC_BASE_URL=http://localhost:8088
    # or
    export OPENAI_API_BASE=http://localhost:8088/v1

Env:
    TRACKWARD_LEDGER_URL    default http://localhost:3000
    TRACKWARD_ACTOR         default model-proxy@$USER
    TRACKWARD_LEDGER_TOKEN  optional bearer

Limits (v1 / MVP):
- Streaming responses (SSE) are forwarded byte-for-byte but the recorded
  `model_response` event contains the assembled body. Live per-chunk
  recording is a v2 item.
- All traffic seen by the proxy maps to a single Trackward run minted
  on first request. Multi-session correlation (per-tab, per-task) is
  v2 — needs a header convention or a session-id query param.
"""

import argparse
import json
import os
import sys
import urllib.request
import urllib.error
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlparse, urlunparse

LEDGER = os.environ.get("TRACKWARD_LEDGER_URL", "http://localhost:3000").rstrip("/")
ACTOR = os.environ.get("TRACKWARD_ACTOR", f"model-proxy@{os.environ.get('USER', 'unknown')}")
TOKEN = os.environ.get("TRACKWARD_LEDGER_TOKEN")

# Single run for the proxy's lifetime — see v2 note above.
_run_id = None


def to_safe(v):
    """Coerce floats: ledger's canonical_json rejects them by design.
    Whole-number floats become int; others become string. Idempotent."""
    if isinstance(v, bool):
        return v
    if isinstance(v, float):
        return int(v) if v.is_integer() else str(v)
    if isinstance(v, dict):
        return {k: to_safe(x) for k, x in v.items()}
    if isinstance(v, list):
        return [to_safe(x) for x in v]
    return v


def _ledger_post(path, body):
    headers = {"x-trackward-actor": ACTOR, "content-type": "application/json"}
    if TOKEN:
        headers["Authorization"] = f"Bearer {TOKEN}"
    data = json.dumps(to_safe(body)).encode()
    req = urllib.request.Request(f"{LEDGER}{path}", data=data, headers=headers, method="POST")
    with urllib.request.urlopen(req, timeout=10) as r:
        txt = r.read().decode()
        return json.loads(txt) if txt else {}


def ensure_run(meta):
    global _run_id
    if _run_id is None:
        run = _ledger_post("/runs", {"agent": "model-proxy", "metadata": meta})
        _run_id = run["id"]
        print(f"[trackward-model-proxy] minted run_id={_run_id}", file=sys.stderr, flush=True)
    return _run_id


def post_event(run_id, kind, body):
    try:
        _ledger_post(f"/runs/{run_id}/events", {"kind": kind, "body": body})
    except Exception as e:
        # Never block the proxy on a ledger failure. Audit-grade installs
        # use TRACKWARD_FAIL_LOUD=1 to switch to hard-stop semantics.
        if os.environ.get("TRACKWARD_FAIL_LOUD"):
            raise
        print(f"[trackward-model-proxy] ledger post {kind} failed: {e}", file=sys.stderr, flush=True)


def make_handler(upstream_base):
    up = urlparse(upstream_base)

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *a):
            pass

        def do_POST(self):
            self._proxy()

        def do_GET(self):
            self._proxy()

        def _proxy(self):
            n = int(self.headers.get("content-length", 0))
            req_body = self.rfile.read(n) if n else b""

            target = urlunparse((up.scheme, up.netloc, self.path, "", "", ""))
            fwd_headers = {k: v for k, v in self.headers.items() if k.lower() != "host"}
            fwd_headers["host"] = up.netloc

            req = urllib.request.Request(
                target,
                data=req_body if req_body else None,
                headers=fwd_headers,
                method=self.command,
            )

            resp_body = b""
            status = 502
            resp_headers = {}
            try:
                with urllib.request.urlopen(req, timeout=300) as up_resp:
                    resp_body = up_resp.read()
                    status = up_resp.status
                    resp_headers = dict(up_resp.headers)
            except urllib.error.HTTPError as e:
                resp_body = e.read()
                status = e.code
                resp_headers = dict(e.headers)
            except Exception as e:
                resp_body = json.dumps({"error": f"upstream: {e}"}).encode()
                status = 502
                resp_headers = {"content-type": "application/json"}

            # Record to ledger best-effort. JSON-decode best-effort.
            try:
                req_json = json.loads(req_body) if req_body else {}
            except Exception:
                req_json = {"_raw_bytes": len(req_body)}
            try:
                resp_json = json.loads(resp_body) if resp_body else {}
            except Exception:
                resp_json = {"_raw_bytes": len(resp_body)}

            try:
                run = ensure_run({"upstream": upstream_base, "first_path": self.path})
                post_event(run, "model_request", {
                    "method": self.command,
                    "path": self.path,
                    "model": req_json.get("model"),
                    "messages": req_json.get("messages"),
                    "tools": req_json.get("tools"),
                    "system": req_json.get("system"),
                    "max_tokens": req_json.get("max_tokens"),
                    "temperature": req_json.get("temperature"),
                })
                post_event(run, "model_response", {
                    "status": status,
                    "id": resp_json.get("id"),
                    "stop_reason": resp_json.get("stop_reason"),
                    "content": resp_json.get("content"),
                    "usage": resp_json.get("usage"),
                    "choices": resp_json.get("choices"),
                })
            except Exception as e:
                if os.environ.get("TRACKWARD_FAIL_LOUD"):
                    raise
                print(f"[trackward-model-proxy] record failed: {e}", file=sys.stderr, flush=True)

            # Forward to client (drop hop-by-hop headers).
            self.send_response(status)
            for k, v in resp_headers.items():
                if k.lower() in ("transfer-encoding", "content-length", "connection"):
                    continue
                self.send_header(k, v)
            self.send_header("content-length", str(len(resp_body)))
            self.end_headers()
            try:
                self.wfile.write(resp_body)
            except BrokenPipeError:
                pass

    return Handler


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--upstream", required=True,
                    help="upstream model API base URL, e.g. https://api.anthropic.com")
    ap.add_argument("--port", type=int, default=8088)
    ap.add_argument("--bind", default="127.0.0.1",
                    help="bind address; default 127.0.0.1 (cleartext local only)")
    args = ap.parse_args()

    handler = make_handler(args.upstream.rstrip("/"))
    server = ThreadingHTTPServer((args.bind, args.port), handler)
    print(f"[trackward-model-proxy] listening on {args.bind}:{args.port} → {args.upstream}",
          file=sys.stderr, flush=True)
    print(f"[trackward-model-proxy] ledger={LEDGER} actor={ACTOR}",
          file=sys.stderr, flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print(f"\n[trackward-model-proxy] shutdown", file=sys.stderr)


if __name__ == "__main__":
    main()
