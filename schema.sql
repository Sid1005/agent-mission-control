-- ============================================================
-- Hermes Mission Control — Agent Mission Control Tables
-- ============================================================

drop table if exists missions        cascade;
drop table if exists mission_tasks   cascade;
drop table if exists mission_activity cascade;
drop table if exists approvals       cascade;
drop table if exists agents          cascade;
drop table if exists cron_jobs       cascade;

-- 1. AGENTS
create table agents (
    id         text primary key,
    name       text not null,
    color      text not null,
    role       text not null default '',
    created_at timestamptz not null default now()
);

-- 2. MISSIONS
create table missions (
    id          text primary key,
    agent       text not null references agents(id),
    session_id  text unique,
    title       text not null,
    status      text not null default 'pending'
                check (status in ('right_now','in_progress','waiting_approval','pending','completed','failed')),
    source      text not null default 'manual',
    ticket_id   text,
    importance  text not null default 'medium',
    horizon     text,
    cwd         text,
    model       text,
    started_at  timestamptz not null default now(),
    ended_at    timestamptz,
    updated_at  timestamptz not null default now()
);

-- 3. MISSION TASKS
create table mission_tasks (
    id         text primary key,
    mission_id text not null references missions(id) on delete cascade,
    title      text not null,
    done       boolean not null default false,
    created_at timestamptz not null default now()
);

-- 4. MISSION ACTIVITY
create table mission_activity (
    id         text primary key,
    mission_id text references missions(id) on delete cascade,
    agent      text not null,
    text       text not null,
    created_at timestamptz not null default now()
);

-- 5. APPROVALS
create table approvals (
    id           text primary key,
    mission_id   text not null references missions(id) on delete cascade,
    agent        text not null,
    description  text not null,
    status       text not null default 'pending' check (status in ('pending','approved','rejected')),
    requested_at timestamptz not null default now(),
    resolved_at  timestamptz
);

-- 6. CRON JOBS
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

-- indexes
create index idx_missions_status       on missions (status);
create index idx_missions_agent        on missions (agent);
create index idx_missions_session_id   on missions (session_id);
create index idx_mission_activity_time on mission_activity (created_at desc);
create index idx_approvals_status      on approvals (status);

-- seed agents
insert into agents (id, name, color, role) values
    ('claude',    'Claude',    '#a78bfa', 'Orchestrator — engineering & coordination'),
    ('codex',     'Codex',     '#6b8cae', 'OpenAI Codex — code generation & review'),
    ('hermes',    'Hermes',    '#c4a35a', 'Hermes — automation & messaging')
on conflict (id) do nothing;
