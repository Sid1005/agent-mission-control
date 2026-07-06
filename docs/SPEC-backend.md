# SPEC — Agent Mission Control Backend (Rust/Axum)

Rewrite `schema.sql` and `src/main.rs`. Keep the existing stack: axum, sqlx (PgPool, runtime queries — NO sqlx macros, no compile-time query checking), tokio, serde, uuid, chrono, dotenvy, tower-http (CorsLayer::permissive + ServeDir("static")). Bind `127.0.0.1:3030`. DATABASE_URL from env/.env.

This replaces the old dashboard (tasks/memories kanban) with **Agent Mission Control**: tracking what AI agents (claude, codex, hermes) are doing.

## schema.sql (full new file; drop-and-create style with `drop table if exists ... cascade` at top for: missions, mission_tasks, mission_activity, approvals, agents, cron_jobs)

```sql
create table agents (
    id    text primary key,              -- 'claude' | 'codex' | 'hermes'
    name  text not null,
    color text not null,
    role  text not null default '',
    created_at timestamptz not null default now()
);

create table missions (
    id          text primary key,        -- 'msn-' || substr(md5(random()::text),1,8) generated in Rust via uuid
    agent       text not null references agents(id),
    session_id  text unique,             -- nullable; hook-driven missions key on this
    title       text not null,
    status      text not null default 'pending'
                check (status in ('right_now','in_progress','waiting_approval','pending','completed','failed')),
    source      text not null default 'manual',   -- 'hook' | 'handoff' | 'manual'
    ticket_id   text,
    importance  text not null default 'medium',   -- low|medium|high|urgent
    horizon     text,
    cwd         text,
    model       text,
    started_at  timestamptz not null default now(),
    ended_at    timestamptz,
    updated_at  timestamptz not null default now()
);

create table mission_tasks (
    id         text primary key,
    mission_id text not null references missions(id) on delete cascade,
    title      text not null,
    done       boolean not null default false,
    created_at timestamptz not null default now()
);

create table mission_activity (
    id         text primary key,
    mission_id text references missions(id) on delete cascade,  -- nullable = global event
    agent      text not null,
    text       text not null,
    created_at timestamptz not null default now()
);

create table approvals (
    id           text primary key,
    mission_id   text not null references missions(id) on delete cascade,
    agent        text not null,
    description  text not null,
    status       text not null default 'pending' check (status in ('pending','approved','rejected')),
    requested_at timestamptz not null default now(),
    resolved_at  timestamptz
);

create table cron_jobs (
    id          text primary key,
    agent       text not null default 'hermes' references agents(id),
    name        text not null,
    schedule    text not null,
    description text not null default '',
    next_run    text not null default '',
    status      text not null default 'scheduled'
                check (status in ('scheduled','running','completed','failed','paused')),
    created_at  timestamptz not null default now()
);
```

Indexes: missions(status), missions(agent), missions(session_id), mission_activity(created_at desc), approvals(status).

Seed (insert ... on conflict (id) do nothing):
- agents: ('claude','Claude','#a78bfa','Orchestrator — engineering & coordination'), ('codex','Codex','#6b8cae','OpenAI Codex — code generation & review'), ('hermes','Hermes','#c4a35a','Hermes — automation & messaging')

## API (all JSON; every handler maps DB errors to 500 with eprintln, like the current code)

Timestamps serialize as RFC3339 strings (use `chrono::DateTime<Utc>` fields; sqlx `timestamptz` → `DateTime<Utc>` works with the `chrono` feature already enabled).

### GET /api/stats
```json
{ "right_now": 1, "in_progress": 2, "waiting_approval": 1, "pending": 0,
  "completed_today": 4, "failed_today": 0, "cron_count": 3 }
```
completed_today/failed_today: `ended_at >= date_trunc('day', now())`.

### GET /api/agents
Array of agents each with live counts:
```json
[{ "id":"claude","name":"Claude","color":"#a78bfa","role":"...",
   "running":1,"queued":0,"waiting_approval":0 }]
```
running = missions in ('right_now','in_progress'); queued = 'pending'; waiting_approval = 'waiting_approval'. One SQL with left join + filtered counts is fine.

### GET /api/missions?status=<s>&agent=<a>&limit=<n default 100>
Missions ordered by updated_at desc. Each mission includes `"tasks": [MissionTask]` (fetch per mission is fine at this scale, or one query grouped in Rust).

### POST /api/missions  { agent, title, status?, importance?, source? } → 201 mission
### PUT /api/missions/:id  { status } → 204; sets updated_at=now(), and ended_at=now() when status in ('completed','failed'). 404 if no row.

### GET /api/activity?limit=<n default 60>
Global activity feed, newest first: `[{id, mission_id, agent, text, created_at}]`.

### GET /api/cron  /  POST /api/cron { agent?, name, schedule, description?, next_run?, status? } → 201
### PUT /api/cron/:id { status?, next_run? } → 204

### GET /api/approvals?status=pending (default pending; `all` returns everything)
Join mission title: `[{id, mission_id, mission_title, agent, description, status, requested_at}]`.

### POST /api/approvals/:id/resolve { status: "approved"|"rejected" } → 204
Sets resolved_at=now(). Also updates the parent mission: approved → status 'right_now'; rejected → 'failed' + ended_at. Insert a mission_activity row ("approval approved: <description first 80 chars>").

### POST /api/handoff  (Dash contract) → 201 mission
Body: `{ ticketId, title, dueDate?, horizon?, importance?, subtasks?: string[], agent }` where agent ∈ codex|claude|hermes. Creates mission (source='handoff', status='pending') + mission_tasks rows + activity row.

### POST /api/events  — THE INGEST ENDPOINT (hooks post here)
Body:
```json
{ "agent": "claude|codex|hermes", "session_id": "…", "event": "…",
  "title": "…?", "detail": "…?", "cwd": "…?", "model": "…?" }
```
Response 200 `{"mission_id": "…"}`.

Logic (single source of truth for hook → board mapping). First, upsert-find the mission by session_id:
- If a mission with this session_id exists, use it. Else insert one: agent, session_id, title = provided title or `"<agent> session"`, status 'right_now', source 'hook', cwd, model.

Then per event:
| event | effect |
|---|---|
| `session_start` | ensure mission exists (status 'right_now'); activity "session started" |
| `prompt` | set status='right_now'; if title provided, and current title is the placeholder OR mission was completed, set title = title (truncate 120 chars); if mission was completed/failed, reopen it (ended_at=null); activity "prompt: <title>" |
| `approval_request` | status='waiting_approval'; insert approvals row (description = detail or title or 'approval requested'); activity |
| `approval_response` | resolve the newest pending approval for this mission (status: detail=='deny'/'rejected' → rejected else approved); mission status='right_now'; activity |
| `turn_end` | status='completed', ended_at=now(); activity "turn completed" (append detail if present) |
| `session_end` | if status not in ('completed','failed'): status='completed', ended_at=now(); activity "session ended" |
| `error` | status='failed', ended_at=now(); activity "error: <detail>" |
| `activity` | just insert activity row with detail/title text |

Always bump missions.updated_at. Unknown agent ids: 400. Unknown event: 400.

## Structure
Keep everything in `src/main.rs` (it's a small service), organized with the same section-banner comment style as the current file. No new crate dependencies unless strictly needed. Must compile with `cargo build`.
