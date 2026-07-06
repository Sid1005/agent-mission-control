# SPEC — mc-report (agent → mission control glue)

Create `bin/mc-report` — a single-file **python3 stdlib-only** executable script (shebang `#!/usr/bin/env python3`, will be chmod +x). It is invoked by agent CLI hooks and POSTs one event to the mission-control ingest endpoint. It must be **fail-silent and fast**: total network timeout 1.5s, wrap everything in try/except, ALWAYS exit 0, never print to stdout unless MC_DEBUG=1 (stderr ok under debug). Server default `http://127.0.0.1:3030`, overridable via env `MC_URL`.

Usage: `mc-report <mode> [event]` where mode ∈ `claude`, `hermes`, `codex`.

POST JSON to `{MC_URL}/api/events`:
`{"agent": ..., "session_id": ..., "event": ..., "title": ...?, "detail": ...?, "cwd": ...?, "model": ...?}`

## mode: claude  — `mc-report claude <event>`
Claude Code hooks pipe a JSON payload on **stdin**. Fields of interest: `session_id`, `cwd`, `prompt` (UserPromptSubmit), `message` (Notification), `hook_event_name`.
- event arg `prompt`  → send event=prompt, title = payload["prompt"] first 120 chars (single line: collapse whitespace).
- event arg `turn_end` → event=turn_end (used for the Stop hook).
- event arg `approval_request` → event=approval_request, detail = payload.get("message"). ONLY send if the message looks like a permission/approval notification: contains any of "permission", "approval", "waiting for your input" (case-insensitive); otherwise exit 0 silently.
- Read stdin fully, json.loads, tolerate malformed input (exit 0).

## mode: hermes — `mc-report hermes <event>`
Hermes shell hooks also pipe JSON on stdin: keys `hook_event_name`, `session_id`, `cwd`, `tool_name`, `extra` (dict). Map the passed event arg directly:
- `session_start` → event=session_start
- `session_end` → event=session_end
- `approval_request` → event=approval_request, detail = extra.get("description") or extra.get("command") or "approval requested"
- `approval_response` → event=approval_response, detail = extra.get("choice","")
If session_id missing, use "hermes-unknown".

## mode: codex — `mc-report codex`
Codex `notify` invokes the program with a single **argv argument** containing JSON (no stdin), e.g. `{"type":"agent-turn-complete","turn-id":"…","input-messages":["…"],"last-assistant-message":"…"}`. Also tolerate the payload appearing at any argv position (scan argv for the first arg that json.loads to a dict).
- Only handle type == "agent-turn-complete"; anything else exit 0.
- session_id = "codex-" + (turn-id or thread-id or hash of input). 
- Send TWO events in sequence: first `prompt` with title = first input message (120 chars), then `turn_end` with detail = last-assistant-message first 200 chars. (This makes each codex turn appear and complete on the board.)

## Shared helpers
- `send(payload)`: urllib.request POST, Content-Type application/json, timeout 1.5.
- Truncate + whitespace-collapse helper.
- If required data missing, best-effort defaults (title None is fine; omit None keys).

Also create `bin/codex-notify` — tiny bash script:
```bash
#!/bin/bash
# forward to the original computer-use notifier, then report to mission control
"/Users/siddharthceri/.codex/computer-use/Codex Computer Use.app/Contents/SharedSupport/SkyComputerUseClient.app/Contents/MacOS/SkyComputerUseClient" turn-ended "$@" >/dev/null 2>&1 &
exec /Users/siddharthceri/Documents/hermes-mission-control/bin/mc-report codex "$@"
```
(exact paths as written; keep the & so the notifier never blocks; mc-report must still exit 0 fast.)
