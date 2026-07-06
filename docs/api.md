# Agent Mission Control API

> Base URL: `http://127.0.0.1:3030/api` — all JSON, permissive CORS. Timestamps are RFC3339 UTC.

## GET /stats
```json
{ "right_now": 1, "in_progress": 0, "waiting_approval": 1, "pending": 2,
  "completed_today": 4, "failed_today": 0, "cron_count": 3 }
```
`completed_today`/`failed_today` count missions with `ended_at` today.

## GET /agents
Agents with live capacity counts (`running` = right_now + in_progress, `queued` = pending):
```json
[{ "id": "claude", "name": "Claude", "color": "#a78bfa",
   "role": "Orchestrator — engineering & coordination",
   "running": 1, "queued": 0, "waiting_approval": 0 }]
```

## GET /missions?status=&agent=&limit=
Missions newest-updated first, each with its subtasks:
```json
[{ "id": "msn-8f13b5d3", "agent": "claude", "session_id": "abc123",
   "title": "Fix the login redirect bug", "status": "right_now",
   "source": "hook", "ticket_id": null, "importance": "medium",
   "horizon": null, "cwd": "/path", "model": null,
   "started_at": "…", "ended_at": null, "updated_at": "…",
   "tasks": [{ "id": "mt-1", "mission_id": "msn-…", "title": "…", "done": false, "created_at": "…" }] }]
```

## POST /missions → 201
Body: `{ "agent", "title", "status"?, "importance"?, "source"? }`. Manual mission creation.

## PUT /missions/:id → 204
Body: `{ "status" }`. Sets `updated_at`; sets `ended_at` when status is `completed`/`failed`. 404 if unknown.

## GET /activity?limit= (default 60)
Global feed, newest first: `[{ "id", "mission_id", "agent", "text", "created_at" }]`.

## GET /cron · POST /cron → 201 · PUT /cron/:id → 204
Cron job: `{ "id", "agent", "name", "schedule", "description", "next_run", "status", "created_at" }` — status ∈ scheduled/running/completed/failed/paused. POST body: `{ agent?, name, schedule, description?, next_run?, status? }`. PUT body: `{ status?, next_run? }`.

## GET /approvals?status=pending|all (default pending)
```json
[{ "id": "apr-…", "mission_id": "msn-…", "mission_title": "…",
   "agent": "claude", "description": "Claude needs your permission to use Bash",
   "status": "pending", "requested_at": "…" }]
```

## POST /approvals/:id/resolve → 204
Body: `{ "status": "approved" | "rejected" }`. Resolves the approval and moves the mission back to `right_now` (approved) or to `failed` (rejected), plus an activity entry.

## POST /handoff → 201 (Dash contract)
```json
{ "ticketId": "DASH-42", "title": "…", "dueDate": "…", "horizon": "this_week",
  "importance": "high", "subtasks": ["…"], "agent": "codex" }
```
Creates a `pending` mission (`source: "handoff"`) with `mission_tasks` rows. Returns the mission.

## POST /events — ingest endpoint (what the hooks call)
```json
{ "agent": "claude|codex|hermes", "session_id": "…", "event": "…",
  "title": "…?", "detail": "…?", "cwd": "…?", "model": "…?" }
```
Returns `{ "mission_id": "msn-…" }`. Upserts the mission by `session_id`, then applies the event:

| event | effect on mission |
|---|---|
| `session_start` | ensure exists, status `right_now` |
| `prompt` | status `right_now`; sets title if placeholder or reopening a completed mission |
| `approval_request` | status `waiting_approval` + pending approvals row |
| `approval_response` | resolves newest pending approval (`detail` = deny/rejected → rejected); status `right_now` |
| `turn_end` | status `completed`, `ended_at` now |
| `session_end` | `completed` unless already completed/failed |
| `error` | status `failed` |
| `activity` | activity row only |

Unknown agents → 400; unknown events → 400. Every event writes a `mission_activity` row.
