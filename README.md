# Agent Mission Control

Agent Mission Control is the control room for my AI agents. Every task any of them runs, **Claude Code**, **Codex**, or **Hermes**, shows up here the moment it starts: what's running right now, what's queued, what's blocked waiting for my approval, and what finished or failed while I wasn't looking.

Dashboard: **http://127.0.0.1:3030** — three tabs: **Tasks / Calendar / Agents**.

## Built with

- `Rust` (Axum + sqlx + Tokio)
- `Postgres` (Neon)
- `Vanilla JS` — one self-contained HTML file, zero build step
- `Python` (stdlib-only ingest shim)
- `launchd`

## Why this exists

I run multiple coding agents in parallel, in different terminals, on different machines. 

So every agent reports into one place. When Claude picks up a prompt, a mission card flips to **Right Now**. When Codex finishes a turn, its card completes. When any agent stops to ask permission, the card lands in **Approval** — which is the column I actually care about, because a blocked agent is a bottleneck.

## Architecture

```
Claude Code hooks ─┐
Hermes shell hooks ─┼→ bin/mc-report ─→ POST /api/events ─→ Axum server ─→ Postgres (Neon)
Codex notify ──────┘                                            │
                                                                └→ static/index.html (polls /api every 5s)
```

Three design decisions carry the whole system:

1. **Agents never talk to the database.** They fire-and-forget JSON at `POST /api/events` through `bin/mc-report` — a stdlib-only Python shim that is *fail-silent and always exits 0*, so a dead dashboard can never break an agent's hook chain. All event→board mapping lives in the server.
2. **Missions are keyed by `session_id`.** Repeated events from one agent session upsert the same card instead of spamming new ones. A `prompt` event can retitle and reopen a completed mission — a session that wakes back up is the same story, not a new card.
3. **The frontend is one static file.** No framework, no bundler. The server serves `static/index.html`, which polls the read API every 5 seconds. Deploying a UI change is `cp`.

### Backend

`src/main.rs` — Axum server bound to `127.0.0.1:3030`, `DATABASE_URL` from `.env`.

Mission lifecycle: `pending → right_now → in_progress → waiting_approval → completed | failed`.

| Event | Effect |
|---|---|
| `session_start` / `prompt` | mission goes **right now** (prompt sets the title) |
| `approval_request` | **waiting approval** + approvals row |
| `approval_response` | approval resolved, mission back to **right now** (or **failed** on reject) |
| `turn_end` / `session_end` | **completed** |
| `error` | **failed** |

Unknown agents and unknown events are rejected with `400` — the roster is `claude`, `codex`, `hermes`, enforced in code, schema, and seed data. Full endpoint reference: [`docs/api.md`](docs/api.md).

### Schema

`schema.sql` — six tables: `agents`, `missions`, `mission_tasks`, `mission_activity`, `approvals`, `cron_jobs`. Approvals are bookkeeping: Approve/Reject on the board records the decision and unblocks the card, but the real CLI prompt still lives in the agent's terminal.

### Agent integrations

| Agent | Wired where | Events |
|---|---|---|
| Claude Code | `~/.claude/settings.json` hooks (async) | `UserPromptSubmit` → prompt · `Stop` → turn_end · `Notification` → approval_request |
| Hermes | `~/.hermes/config.yaml` `hooks:` | session start/end, first LLM call titles the mission, approval request/response |
| Codex | `~/.codex/config.toml` `notify` → `bin/codex-notify` | each `agent-turn-complete` → prompt + turn_end |

## Running it

```bash
# 1. database
psql "$DATABASE_URL" -f schema.sql

# 2. server
echo 'DATABASE_URL=postgres://…' > .env
cargo run --release          # serves http://127.0.0.1:3030

# 3. wire your agents' hooks at bin/mc-report (see table above)
```

On macOS I keep it permanent via launchd, with `bin/deploy` doing build → install → restart:

```bash
bin/deploy        # cargo build, copy binary+static to ~/.local/opt/mission-control, restart launchd
tail -f ~/Library/Logs/mission-control.log
```

> **Gotcha that cost an evening:** launchd cannot exec a binary that lives inside `~/Documents` — macOS TCC stalls the spawn *inside dyld*, forever, with no error anywhere. The process shows as running but never reaches `main`. That's why the runtime is deployed to `~/.local/opt/mission-control` instead of running from `target/`. Relatedly, the DB pool is `connect_lazy` so a slow DNS answer (IPv6-first on an IPv4-only network) can never keep the listener from binding.

## Repo map

```
src/main.rs          Axum server — routes, event→board mapping, sqlx queries
schema.sql           tables + seed agents (claude / codex / hermes)
static/index.html    the whole frontend
bin/mc-report        hook payload → POST /api/events (python3, stdlib, fail-silent)
bin/codex-notify     Codex notify adapter → mc-report
docs/api.md          endpoint reference
docs/SPEC-*.md       original build specs (backend / frontend / reporter)
```
