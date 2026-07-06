# SPEC — Agent Mission Control Frontend

Rewrite `static/index.html` as one self-contained file (inline CSS/JS, no external deps).

**Visual base:** copy the look, CSS variables, layout and components of `agent-mission-control-demo/index.html` (read it fully first — dark theme, topbar with tab nav, three views: Tasks / Calendar / Agents, agent signature colors --codex/--claude/--hermes/--open). The demo uses hardcoded mock data; your job is to keep its shell and visual language but drive everything from the live API.

**API base:** same origin, `/api`. See `docs/SPEC-backend.md` for exact shapes. Poll every 5 seconds (single `refresh()` that fetches stats, missions, agents, activity, cron, approvals in parallel and re-renders; also refresh immediately after any user action). Show a subtle "live" status dot in the topbar; if a fetch fails, show "offline" in the topbar instead of crashing.

Agent color map in JS: `{codex:'#6b8cae', claude:'#a78bfa', hermes:'#c4a35a'}`. Every mission card, calendar job, activity row, and agent card shows the agent's color as a left border / dot PLUS the agent name label (never color alone).

## Tasks tab
- Top stats row from `GET /api/stats`: **Right now**, **In progress**, **Waiting approval** (waiting approval styled prominently — accent/danger tint when > 0).
- Board with three columns fed by `GET /api/missions`:
  - `Right now` → status right_now
  - `In progress` → status in_progress + pending (pending cards get a small "queued" pill)
  - `Approval` → status waiting_approval
- Mission card: title, agent pill (color + name), importance pill, relative time since updated_at ("2m ago"), and if the mission has subtasks show "3/5 tasks". Cards in Approval column render the pending approval description (from `GET /api/approvals?status=pending`, matched by mission_id) and two buttons: **Approve** / **Reject** → `POST /api/approvals/:id/resolve {"status":"approved"|"rejected"}` then refresh.
- Below the board: **Recently completed** — last ~8 missions with status completed/failed (failed in danger color), with ended time.
- Activity tail: `GET /api/activity?limit=25` — time (HH:MM from created_at), agent dot+name, text.
- Empty states: friendly muted text ("Nothing running right now").

## Calendar tab
Operational jobs only, from `GET /api/cron`. List grouped rows like the demo: time/schedule (mono font), agent dot+name, title, description, status pill (`running now` pulsing, `scheduled`, `queued`, `completed`, `failed`, `paused`). Sort: running first, then scheduled. Empty state if none.

## Agents tab
From `GET /api/agents`: card per agent like the demo — monogram avatar in signature color, name, role, and three counts: running / queued / waiting approval. Waiting approval count highlighted when > 0. Also show a tiny footer line per agent: last activity text for that agent if available from the activity feed.

## Notes
- Vanilla JS only, no framework. `esc()` helper to HTML-escape all API strings before injecting into innerHTML.
- Relative-time helper for "Xs/Xm/Xh ago".
- Keep the page title "Agent Mission Control" and the demo's header/eyebrow styling.
- Must work by just serving the file — no build step.
