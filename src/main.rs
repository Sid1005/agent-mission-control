use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
    routing::{get, post, put},
    Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use sqlx::PgPool;
use sqlx::QueryBuilder;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use uuid::Uuid;

// ============================================================================
// 1. DATA MODELS
// ============================================================================

#[derive(Serialize, Deserialize, FromRow, Clone, Debug)]
struct Agent {
    id: String,
    name: String,
    color: String,
    role: String,
    created_at: DateTime<Utc>,
}

#[derive(Serialize, Debug)]
struct AgentWithCounts {
    id: String,
    name: String,
    color: String,
    role: String,
    running: i64,
    queued: i64,
    waiting_approval: i64,
}

#[derive(Serialize, Deserialize, FromRow, Clone, Debug)]
struct AgentCounts {
    id: String,
    name: String,
    color: String,
    role: String,
    created_at: DateTime<Utc>,
    running: i64,
    queued: i64,
    waiting_approval: i64,
}

#[derive(Serialize, Deserialize, FromRow, Clone, Debug)]
struct MissionRow {
    id: String,
    agent: String,
    session_id: Option<String>,
    title: String,
    status: String,
    source: String,
    ticket_id: Option<String>,
    importance: String,
    horizon: Option<String>,
    cwd: Option<String>,
    model: Option<String>,
    started_at: DateTime<Utc>,
    ended_at: Option<DateTime<Utc>>,
    updated_at: DateTime<Utc>,
}

#[derive(Serialize, Debug)]
struct MissionResponse {
    id: String,
    agent: String,
    session_id: Option<String>,
    title: String,
    status: String,
    source: String,
    ticket_id: Option<String>,
    importance: String,
    horizon: Option<String>,
    cwd: Option<String>,
    model: Option<String>,
    started_at: DateTime<Utc>,
    ended_at: Option<DateTime<Utc>>,
    updated_at: DateTime<Utc>,
    tasks: Vec<MissionTask>,
}

#[derive(Serialize, Deserialize, FromRow, Clone, Debug)]
struct MissionTask {
    id: String,
    mission_id: String,
    title: String,
    done: bool,
    created_at: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, FromRow, Clone, Debug)]
struct MissionActivity {
    id: String,
    mission_id: Option<String>,
    agent: String,
    text: String,
    created_at: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, FromRow, Clone, Debug)]
struct Approval {
    id: String,
    mission_id: String,
    agent: String,
    description: String,
    status: String,
    requested_at: DateTime<Utc>,
    resolved_at: Option<DateTime<Utc>>,
}

#[derive(Serialize, FromRow, Debug)]
struct ApprovalWithMission {
    id: String,
    mission_id: String,
    mission_title: String,
    agent: String,
    description: String,
    status: String,
    requested_at: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, FromRow, Clone, Debug)]
struct CronJob {
    id: String,
    agent: String,
    name: String,
    schedule: String,
    description: String,
    next_run: String,
    status: String,
    created_at: DateTime<Utc>,
}

// --- stats response ---

#[derive(Serialize, Debug)]
struct Stats {
    right_now: i64,
    in_progress: i64,
    waiting_approval: i64,
    pending: i64,
    completed_today: i64,
    failed_today: i64,
    cron_count: i64,
}

// --- request bodies ---

#[derive(Deserialize, Debug)]
struct MissionQuery {
    status: Option<String>,
    agent: Option<String>,
    #[serde(default = "default_limit")]
    limit: i64,
}
fn default_limit() -> i64 {
    100
}

#[derive(Deserialize, Debug)]
struct ActivityQuery {
    #[serde(default = "default_activity_limit")]
    limit: i64,
}
fn default_activity_limit() -> i64 {
    60
}

#[derive(Deserialize, Debug)]
struct ApprovalQuery {
    #[serde(default = "default_approval_status")]
    status: String,
}
fn default_approval_status() -> String {
    String::from("pending")
}

#[derive(Deserialize, Debug)]
struct CreateMission {
    agent: String,
    title: String,
    #[serde(default = "default_mission_status")]
    status: String,
    #[serde(default = "default_importance")]
    importance: String,
    #[serde(default = "default_source")]
    source: String,
    horizon: Option<String>,
    cwd: Option<String>,
    model: Option<String>,
    ticket_id: Option<String>,
}
fn default_mission_status() -> String {
    String::from("pending")
}
fn default_source() -> String {
    String::from("manual")
}

#[derive(Deserialize, Debug)]
struct UpdateMission {
    status: String,
}

#[derive(Deserialize, Debug)]
struct CreateCronJob {
    #[serde(default = "default_cron_agent")]
    agent: String,
    name: String,
    schedule: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    next_run: Option<String>,
    #[serde(default = "default_cron_status")]
    status: String,
}
fn default_cron_agent() -> String {
    String::from("hermes")
}
fn default_cron_status() -> String {
    String::from("scheduled")
}

#[derive(Deserialize, Debug)]
struct UpdateCronJob {
    status: Option<String>,
    next_run: Option<String>,
}

#[derive(Deserialize, Debug)]
struct ResolveApproval {
    status: String,
}

#[derive(Deserialize, Debug)]
struct HandoffRequest {
    #[serde(rename = "ticketId")]
    ticket_id: String,
    title: String,
    #[serde(rename = "dueDate")]
    due_date: Option<String>,
    horizon: Option<String>,
    #[serde(default = "default_importance")]
    importance: String,
    subtasks: Option<Vec<String>>,
    agent: String,
}

#[derive(Serialize, Debug)]
struct EventResponse {
    mission_id: String,
}

#[derive(Deserialize, Debug)]
struct IngestEvent {
    agent: String,
    session_id: String,
    event: String,
    title: Option<String>,
    detail: Option<String>,
    cwd: Option<String>,
    model: Option<String>,
}

fn default_importance() -> String {
    String::from("medium")
}

// ============================================================================
// 2. APPLICATION STATE
// ============================================================================

#[derive(Clone)]
struct AppState {
    pool: PgPool,
}

// ============================================================================
// 3. MAIN
// ============================================================================

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let database_url =
        std::env::var("DATABASE_URL").expect("DATABASE_URL must be set in .env");

    // Lazy pool: bind the listener immediately and connect on first query.
    // An eager connect can hang for minutes at boot when DNS returns
    // unreachable IPv6 addresses first, leaving the dashboard dark.
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(std::time::Duration::from_secs(15))
        .connect_lazy(&database_url)
        .expect("Invalid DATABASE_URL");

    let state = AppState { pool };

    let app = Router::new()
        .route("/api/stats", get(get_stats))
        .route("/api/agents", get(get_agents))
        .route("/api/missions", get(get_missions).post(create_mission))
        .route("/api/missions/:id", put(update_mission))
        .route("/api/activity", get(get_activity))
        .route("/api/cron", get(get_cron_jobs).post(create_cron_job))
        .route("/api/cron/:id", put(update_cron_job))
        .route("/api/approvals", get(get_approvals))
        .route("/api/approvals/:id/resolve", post(resolve_approval))
        .route("/api/handoff", post(handle_handoff))
        .route("/api/events", post(handle_events))
        .nest_service("/", ServeDir::new("static"))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3030")
        .await
        .expect("Failed to bind port 3030");

    println!("Hermes Mission Control running at http://127.0.0.1:3030");
    axum::serve(listener, app).await.unwrap();
}

// ============================================================================
// 4. GET HANDLERS
// ============================================================================

async fn get_stats(State(state): State<AppState>) -> Result<Json<Stats>, StatusCode> {
    // One round-trip: the pool talks to a remote Neon instance, so per-query
    // network latency dominates — never issue sequential queries here.
    let row: (i64, i64, i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT
            COUNT(*) FILTER (WHERE status = 'right_now'),
            COUNT(*) FILTER (WHERE status = 'in_progress'),
            COUNT(*) FILTER (WHERE status = 'waiting_approval'),
            COUNT(*) FILTER (WHERE status = 'pending'),
            COUNT(*) FILTER (WHERE status = 'completed' AND ended_at >= date_trunc('day', now())),
            COUNT(*) FILTER (WHERE status = 'failed' AND ended_at >= date_trunc('day', now())),
            (SELECT COUNT(*) FROM cron_jobs)
         FROM missions",
    )
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        eprintln!("DB error: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(Stats {
        right_now: row.0,
        in_progress: row.1,
        waiting_approval: row.2,
        pending: row.3,
        completed_today: row.4,
        failed_today: row.5,
        cron_count: row.6,
    }))
}

async fn get_agents(
    State(state): State<AppState>,
) -> Result<Json<Vec<AgentWithCounts>>, StatusCode> {
    let rows = sqlx::query_as::<_, AgentCounts>(
        "SELECT a.id, a.name, a.color, a.role, a.created_at,
                COALESCE(COUNT(m.id) FILTER (WHERE m.status IN ('right_now','in_progress')), 0) AS running,
                COALESCE(COUNT(m.id) FILTER (WHERE m.status = 'pending'), 0) AS queued,
                COALESCE(COUNT(m.id) FILTER (WHERE m.status = 'waiting_approval'), 0) AS waiting_approval
         FROM agents a
         LEFT JOIN missions m ON m.agent = a.id
         GROUP BY a.id
         ORDER BY a.id",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        eprintln!("DB error: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let agents = rows
        .into_iter()
        .map(|r| AgentWithCounts {
            id: r.id,
            name: r.name,
            color: r.color,
            role: r.role,
            running: r.running,
            queued: r.queued,
            waiting_approval: r.waiting_approval,
        })
        .collect();
    Ok(Json(agents))
}

async fn get_missions(
    State(state): State<AppState>,
    Query(q): Query<MissionQuery>,
) -> Result<Json<Vec<MissionResponse>>, StatusCode> {
    let mut builder = QueryBuilder::new(
        "SELECT id, agent, session_id, title, status, source, ticket_id, importance, horizon, cwd, model, started_at, ended_at, updated_at FROM missions WHERE 1=1",
    );

    if let Some(ref s) = q.status {
        builder.push(" AND status = ");
        builder.push_bind(s);
    }
    if let Some(ref a) = q.agent {
        builder.push(" AND agent = ");
        builder.push_bind(a);
    }

    builder.push(" ORDER BY updated_at DESC LIMIT ");
    builder.push_bind(q.limit);

    let missions: Vec<MissionRow> = builder
        .build_query_as()
        .fetch_all(&state.pool)
        .await
        .map_err(|e| {
            eprintln!("DB error: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    // Fetch tasks for every returned mission in one round-trip (Neon is
    // remote — a per-mission query here previously cost ~27s for 100 rows).
    let ids: Vec<String> = missions.iter().map(|m| m.id.clone()).collect();
    let all_tasks: Vec<MissionTask> = sqlx::query_as(
        "SELECT id, mission_id, title, done, created_at FROM mission_tasks WHERE mission_id = ANY($1) ORDER BY created_at",
    )
    .bind(&ids)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        eprintln!("DB error: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let mut tasks_by_mission: std::collections::HashMap<String, Vec<MissionTask>> =
        std::collections::HashMap::new();
    for t in all_tasks {
        tasks_by_mission.entry(t.mission_id.clone()).or_default().push(t);
    }

    let mut responses = Vec::with_capacity(missions.len());
    for m in missions {
        let tasks = tasks_by_mission.remove(&m.id).unwrap_or_default();

        responses.push(MissionResponse {
            id: m.id,
            agent: m.agent,
            session_id: m.session_id,
            title: m.title,
            status: m.status,
            source: m.source,
            ticket_id: m.ticket_id,
            importance: m.importance,
            horizon: m.horizon,
            cwd: m.cwd,
            model: m.model,
            started_at: m.started_at,
            ended_at: m.ended_at,
            updated_at: m.updated_at,
            tasks,
        });
    }
    Ok(Json(responses))
}

async fn get_activity(
    State(state): State<AppState>,
    Query(q): Query<ActivityQuery>,
) -> Result<Json<Vec<MissionActivity>>, StatusCode> {
    let log = sqlx::query_as::<_, MissionActivity>(
        "SELECT id, mission_id, agent, text, created_at FROM mission_activity ORDER BY created_at DESC LIMIT $1",
    )
    .bind(q.limit)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        eprintln!("DB error: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(Json(log))
}

async fn get_cron_jobs(
    State(state): State<AppState>,
) -> Result<Json<Vec<CronJob>>, StatusCode> {
    let jobs = sqlx::query_as::<_, CronJob>(
        "SELECT id, agent, name, schedule, description, next_run, status, created_at FROM cron_jobs ORDER BY created_at",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        eprintln!("DB error: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(Json(jobs))
}

async fn get_approvals(
    State(state): State<AppState>,
    Query(q): Query<ApprovalQuery>,
) -> Result<Json<Vec<ApprovalWithMission>>, StatusCode> {
    if q.status == "all" {
        let rows = sqlx::query_as::<_, ApprovalWithMission>(
            "SELECT a.id, a.mission_id, m.title AS mission_title, a.agent, a.description, a.status, a.requested_at
             FROM approvals a
             JOIN missions m ON m.id = a.mission_id
             ORDER BY a.requested_at DESC",
        )
        .fetch_all(&state.pool)
        .await
        .map_err(|e| {
            eprintln!("DB error: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
        Ok(Json(rows))
    } else {
        let rows = sqlx::query_as::<_, ApprovalWithMission>(
            "SELECT a.id, a.mission_id, m.title AS mission_title, a.agent, a.description, a.status, a.requested_at
             FROM approvals a
             JOIN missions m ON m.id = a.mission_id
             WHERE a.status = $1
             ORDER BY a.requested_at DESC",
        )
        .bind(&q.status)
        .fetch_all(&state.pool)
        .await
        .map_err(|e| {
            eprintln!("DB error: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
        Ok(Json(rows))
    }
}

// ============================================================================
// 5. POST HANDLERS
// ============================================================================

async fn create_mission(
    State(state): State<AppState>,
    Json(payload): Json<CreateMission>,
) -> Result<(StatusCode, Json<MissionRow>), StatusCode> {
    let id = format!(
        "msn-{}",
        Uuid::new_v4().to_string().split('-').next().unwrap_or("?")
    );

    if !is_known_agent(&payload.agent) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let agent = payload.agent.as_str();

    let mission = sqlx::query_as::<_, MissionRow>(
        "INSERT INTO missions (id, agent, title, status, importance, source, ticket_id, horizon, cwd, model)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         RETURNING id, agent, session_id, title, status, source, ticket_id, importance, horizon, cwd, model, started_at, ended_at, updated_at",
    )
    .bind(&id)
    .bind(agent)
    .bind(&payload.title)
    .bind(&payload.status)
    .bind(&payload.importance)
    .bind(&payload.source)
    .bind(&payload.ticket_id)
    .bind(&payload.horizon)
    .bind(&payload.cwd)
    .bind(&payload.model)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        eprintln!("DB error: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok((StatusCode::CREATED, Json(mission)))
}

async fn create_cron_job(
    State(state): State<AppState>,
    Json(payload): Json<CreateCronJob>,
) -> Result<(StatusCode, Json<CronJob>), StatusCode> {
    let id = Uuid::new_v4().to_string();
    let next_run = payload.next_run.unwrap_or_else(|| {
        Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string()
    });

    let job = sqlx::query_as::<_, CronJob>(
        "INSERT INTO cron_jobs (id, agent, name, schedule, description, next_run, status)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         RETURNING id, agent, name, schedule, description, next_run, status, created_at",
    )
    .bind(&id)
    .bind(&payload.agent)
    .bind(&payload.name)
    .bind(&payload.schedule)
    .bind(&payload.description)
    .bind(&next_run)
    .bind(&payload.status)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        eprintln!("DB error: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok((StatusCode::CREATED, Json(job)))
}

async fn resolve_approval(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<ResolveApproval>,
) -> Result<StatusCode, StatusCode> {
    let approved = body.status == "approved";

    let approval: Approval = sqlx::query_as::<_, Approval>(
        "UPDATE approvals SET status = $1, resolved_at = now() WHERE id = $2
         RETURNING id, mission_id, agent, description, status, requested_at, resolved_at",
    )
    .bind(&body.status)
    .bind(&id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        eprintln!("DB error: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?
    .ok_or(StatusCode::NOT_FOUND)?;

    let new_status = if approved { "right_now" } else { "failed" };
    if approved {
        sqlx::query("UPDATE missions SET status = $1, updated_at = now() WHERE id = $2")
            .bind(new_status)
            .bind(&approval.mission_id)
            .execute(&state.pool)
            .await
            .map_err(|e| {
                eprintln!("DB error: {e}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
    } else {
        sqlx::query(
            "UPDATE missions SET status = $1, ended_at = now(), updated_at = now() WHERE id = $2",
        )
        .bind(new_status)
        .bind(&approval.mission_id)
        .execute(&state.pool)
        .await
        .map_err(|e| {
            eprintln!("DB error: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    }

    let desc = truncate_str(&approval.description, 80);
    let activity_id = format!(
        "act-{}",
        Uuid::new_v4().to_string().split('-').next().unwrap_or("?")
    );
    let activity_text = format!("approval {}: {}", body.status, desc);
    sqlx::query(
        "INSERT INTO mission_activity (id, mission_id, agent, text) VALUES ($1, $2, $3, $4)",
    )
    .bind(&activity_id)
    .bind(&approval.mission_id)
    .bind(&approval.agent)
    .bind(&activity_text)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        eprintln!("DB error: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(StatusCode::NO_CONTENT)
}

async fn handle_handoff(
    State(state): State<AppState>,
    Json(payload): Json<HandoffRequest>,
) -> Result<(StatusCode, Json<MissionRow>), StatusCode> {
    let id = format!(
        "msn-{}",
        Uuid::new_v4().to_string().split('-').next().unwrap_or("?")
    );
    if !is_known_agent(&payload.agent) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let agent = payload.agent.as_str();

    let mission = sqlx::query_as::<_, MissionRow>(
        "INSERT INTO missions (id, agent, title, status, source, ticket_id, importance, horizon)
         VALUES ($1, $2, $3, 'pending', 'handoff', $4, $5, $6)
         RETURNING id, agent, session_id, title, status, source, ticket_id, importance, horizon, cwd, model, started_at, ended_at, updated_at",
    )
    .bind(&id)
    .bind(agent)
    .bind(&payload.title)
    .bind(&payload.ticket_id)
    .bind(&payload.importance)
    .bind(&payload.horizon)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        eprintln!("DB error: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    if let Some(ref subtasks) = payload.subtasks {
        for task_title in subtasks {
            let task_id = format!(
                "mt-{}",
                Uuid::new_v4().to_string().split('-').next().unwrap_or("?")
            );
            sqlx::query(
                "INSERT INTO mission_tasks (id, mission_id, title) VALUES ($1, $2, $3)",
            )
            .bind(&task_id)
            .bind(&id)
            .bind(task_title)
            .execute(&state.pool)
            .await
            .map_err(|e| {
                eprintln!("DB error: {e}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
        }
    }

    let activity_id = format!(
        "act-{}",
        Uuid::new_v4().to_string().split('-').next().unwrap_or("?")
    );
    sqlx::query(
        "INSERT INTO mission_activity (id, mission_id, agent, text) VALUES ($1, $2, $3, $4)",
    )
    .bind(&activity_id)
    .bind(&id)
    .bind(agent)
    .bind(&format!("handoff: {}", &payload.title))
    .execute(&state.pool)
    .await
    .map_err(|e| {
        eprintln!("DB error: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok((StatusCode::CREATED, Json(mission)))
}

async fn handle_events(
    State(state): State<AppState>,
    Json(payload): Json<IngestEvent>,
) -> Result<Json<EventResponse>, StatusCode> {
    if !is_known_agent(&payload.agent) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let agent = payload.agent.as_str();

    let existing: Option<MissionRow> = sqlx::query_as::<_, MissionRow>(
        "SELECT id, agent, session_id, title, status, source, ticket_id, importance, horizon, cwd, model, started_at, ended_at, updated_at
         FROM missions WHERE session_id = $1",
    )
    .bind(&payload.session_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        eprintln!("DB error: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let mission = if let Some(m) = existing {
        m
    } else {
        let mid = format!(
            "msn-{}",
            Uuid::new_v4().to_string().split('-').next().unwrap_or("?")
        );
        let title = payload
            .title
            .clone()
            .unwrap_or_else(|| format!("{} session", agent));
        sqlx::query_as::<_, MissionRow>(
            "INSERT INTO missions (id, agent, session_id, title, status, source, cwd, model)
             VALUES ($1, $2, $3, $4, 'right_now', 'hook', $5, $6)
             RETURNING id, agent, session_id, title, status, source, ticket_id, importance, horizon, cwd, model, started_at, ended_at, updated_at",
        )
        .bind(&mid)
        .bind(agent)
        .bind(&payload.session_id)
        .bind(&title)
        .bind(&payload.cwd)
        .bind(&payload.model)
        .fetch_one(&state.pool)
        .await
        .map_err(|e| {
            eprintln!("DB error: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
    };

    let mission_id = mission.id.clone();
    match payload.event.as_str() {
        "session_start" => {
            sqlx::query(
                "UPDATE missions SET status = 'right_now', updated_at = now() WHERE id = $1",
            )
            .bind(&mission_id)
            .execute(&state.pool)
            .await
            .map_err(|e| {
                eprintln!("DB error: {e}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
            insert_activity(&state.pool, &mission_id, agent, "session started").await?;
        }
        "prompt" => {
            let mission_was_completed = mission.status == "completed";
            let mission_was_done = mission_was_completed || mission.status == "failed";
            let placeholder = format!("{} session", agent);

            if let Some(ref new_title) = payload.title {
                if mission.title == placeholder || mission_was_completed {
                    let truncated = truncate_str(new_title, 120);
                    sqlx::query("UPDATE missions SET title = $1 WHERE id = $2")
                        .bind(&truncated)
                        .bind(&mission_id)
                        .execute(&state.pool)
                        .await
                        .map_err(|e| {
                            eprintln!("DB error: {e}");
                            StatusCode::INTERNAL_SERVER_ERROR
                        })?;
                }
            }

            if mission_was_done {
                sqlx::query("UPDATE missions SET ended_at = NULL WHERE id = $1")
                    .bind(&mission_id)
                    .execute(&state.pool)
                    .await
                    .map_err(|e| {
                        eprintln!("DB error: {e}");
                        StatusCode::INTERNAL_SERVER_ERROR
                    })?;
            }

            sqlx::query(
                "UPDATE missions SET status = 'right_now', updated_at = now() WHERE id = $1",
            )
            .bind(&mission_id)
            .execute(&state.pool)
            .await
            .map_err(|e| {
                eprintln!("DB error: {e}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;

            let prompt_title = payload
                .title
                .as_deref()
                .unwrap_or(&payload.detail.as_deref().unwrap_or(""));
            insert_activity(
                &state.pool,
                &mission_id,
                agent,
                &format!("prompt: {}", prompt_title),
            )
            .await?;
        }
        "approval_request" => {
            sqlx::query(
                "UPDATE missions SET status = 'waiting_approval', updated_at = now() WHERE id = $1",
            )
            .bind(&mission_id)
            .execute(&state.pool)
            .await
            .map_err(|e| {
                eprintln!("DB error: {e}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;

            let desc = payload
                .detail
                .as_deref()
                .or(payload.title.as_deref())
                .unwrap_or("approval requested");
            let approval_id = format!(
                "apr-{}",
                Uuid::new_v4().to_string().split('-').next().unwrap_or("?")
            );
            sqlx::query(
                "INSERT INTO approvals (id, mission_id, agent, description) VALUES ($1, $2, $3, $4)",
            )
            .bind(&approval_id)
            .bind(&mission_id)
            .bind(agent)
            .bind(desc)
            .execute(&state.pool)
            .await
            .map_err(|e| {
                eprintln!("DB error: {e}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;

            insert_activity(
                &state.pool,
                &mission_id,
                agent,
                &format!("approval requested: {}", desc),
            )
            .await?;
        }
        "approval_response" => {
            let resolved_status = match payload.detail.as_deref() {
                Some("deny") | Some("rejected") => "rejected",
                _ => "approved",
            };

            let pending: Option<String> = sqlx::query_scalar(
                "SELECT id FROM approvals WHERE mission_id = $1 AND status = 'pending' ORDER BY requested_at DESC LIMIT 1",
            )
            .bind(&mission_id)
            .fetch_optional(&state.pool)
            .await
            .map_err(|e| {
                eprintln!("DB error: {e}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;

            if let Some(approval_id) = pending {
                sqlx::query("UPDATE approvals SET status = $1, resolved_at = now() WHERE id = $2")
                    .bind(resolved_status)
                    .bind(&approval_id)
                    .execute(&state.pool)
                    .await
                    .map_err(|e| {
                        eprintln!("DB error: {e}");
                        StatusCode::INTERNAL_SERVER_ERROR
                    })?;
            }

            sqlx::query(
                "UPDATE missions SET status = 'right_now', updated_at = now() WHERE id = $1",
            )
            .bind(&mission_id)
            .execute(&state.pool)
            .await
            .map_err(|e| {
                eprintln!("DB error: {e}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;

            insert_activity(
                &state.pool,
                &mission_id,
                agent,
                &format!("approval {}", resolved_status),
            )
            .await?;
        }
        "turn_end" => {
            sqlx::query(
                "UPDATE missions SET status = 'completed', ended_at = now(), updated_at = now() WHERE id = $1",
            )
            .bind(&mission_id)
            .execute(&state.pool)
            .await
            .map_err(|e| {
                eprintln!("DB error: {e}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;

            let mut msg = String::from("turn completed");
            if let Some(ref d) = payload.detail {
                msg.push_str(&format!(": {}", d));
            }
            insert_activity(&state.pool, &mission_id, agent, &msg).await?;
        }
        "session_end" => {
            if mission.status != "completed" && mission.status != "failed" {
                sqlx::query(
                    "UPDATE missions SET status = 'completed', ended_at = now(), updated_at = now() WHERE id = $1",
                )
                .bind(&mission_id)
                .execute(&state.pool)
                .await
                .map_err(|e| {
                    eprintln!("DB error: {e}");
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;
            } else {
                sqlx::query("UPDATE missions SET updated_at = now() WHERE id = $1")
                    .bind(&mission_id)
                    .execute(&state.pool)
                    .await
                    .map_err(|e| {
                        eprintln!("DB error: {e}");
                        StatusCode::INTERNAL_SERVER_ERROR
                    })?;
            }
            insert_activity(&state.pool, &mission_id, agent, "session ended").await?;
        }
        "error" => {
            sqlx::query(
                "UPDATE missions SET status = 'failed', ended_at = now(), updated_at = now() WHERE id = $1",
            )
            .bind(&mission_id)
            .execute(&state.pool)
            .await
            .map_err(|e| {
                eprintln!("DB error: {e}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;

            let detail = payload.detail.as_deref().unwrap_or("");
            insert_activity(&state.pool, &mission_id, agent, &format!("error: {}", detail))
                .await?;
        }
        "activity" => {
            let text = payload
                .detail
                .as_deref()
                .or(payload.title.as_deref())
                .unwrap_or("");
            insert_activity(&state.pool, &mission_id, agent, text).await?;
        }
        _ => {
            return Err(StatusCode::BAD_REQUEST);
        }
    }

    Ok(Json(EventResponse { mission_id }))
}

// ============================================================================
// 6. PUT HANDLERS
// ============================================================================

async fn update_mission(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(update): Json<UpdateMission>,
) -> Result<StatusCode, StatusCode> {
    let is_terminal = update.status == "completed" || update.status == "failed";

    let result = if is_terminal {
        sqlx::query(
            "UPDATE missions SET status = $1, ended_at = now(), updated_at = now() WHERE id = $2",
        )
        .bind(&update.status)
        .bind(&id)
        .execute(&state.pool)
        .await
        .map_err(|e| {
            eprintln!("DB error: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
    } else {
        sqlx::query("UPDATE missions SET status = $1, updated_at = now() WHERE id = $2")
            .bind(&update.status)
            .bind(&id)
            .execute(&state.pool)
            .await
            .map_err(|e| {
                eprintln!("DB error: {e}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?
    };

    if result.rows_affected() > 0 {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

async fn update_cron_job(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(update): Json<UpdateCronJob>,
) -> Result<StatusCode, StatusCode> {
    if let Some(ref status) = update.status {
        let result = sqlx::query("UPDATE cron_jobs SET status = $1 WHERE id = $2")
            .bind(status)
            .bind(&id)
            .execute(&state.pool)
            .await
            .map_err(|e| {
                eprintln!("DB error: {e}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
        if result.rows_affected() == 0 {
            return Err(StatusCode::NOT_FOUND);
        }
    }
    if let Some(ref next_run) = update.next_run {
        let result = sqlx::query("UPDATE cron_jobs SET next_run = $1 WHERE id = $2")
            .bind(next_run)
            .bind(&id)
            .execute(&state.pool)
            .await
            .map_err(|e| {
                eprintln!("DB error: {e}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
        if result.rows_affected() == 0 {
            return Err(StatusCode::NOT_FOUND);
        }
    }
    Ok(StatusCode::NO_CONTENT)
}

// ============================================================================
// 7. HELPERS
// ============================================================================

async fn insert_activity(
    pool: &PgPool,
    mission_id: &str,
    agent: &str,
    text: &str,
) -> Result<(), StatusCode> {
    let id = format!(
        "act-{}",
        Uuid::new_v4().to_string().split('-').next().unwrap_or("?")
    );
    sqlx::query(
        "INSERT INTO mission_activity (id, mission_id, agent, text) VALUES ($1, $2, $3, $4)",
    )
    .bind(&id)
    .bind(mission_id)
    .bind(agent)
    .bind(text)
    .execute(pool)
    .await
    .map_err(|e| {
        eprintln!("DB error: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(())
}

fn truncate_str(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let mut end = max;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        s[..end].to_string()
    }
}

fn is_known_agent(agent: &str) -> bool {
    matches!(agent, "claude" | "codex" | "hermes")
}
