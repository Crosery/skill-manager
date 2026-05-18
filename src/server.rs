//! HTTP dashboard for router telemetry.
//!
//! Spawned by `runai server [--port N] [--host H]`. Reads `~/.runai/runai.db`
//! and serves a single-page HTML dashboard plus JSON endpoints so users can
//! inspect every hook invocation: the user prompt, cwd, chosen skills, BM25
//! prefilter ratio, latency and token usage.
//!
//! No external CDN — index.html / app.js / app.css are bundled via
//! `include_str!` so the dashboard works offline (same single-binary
//! philosophy as the rest of runai).

use anyhow::{Context, Result, bail};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use crate::core::db::{Database, RouterEvent};
use crate::core::manager::SkillManager;
use crate::core::paths::AppPaths;
use crate::core::recommend;

const INDEX_HTML: &str = include_str!("../web/index.html");
const APP_JS: &str = include_str!("../web/app.js");
const APP_CSS: &str = include_str!("../web/app.css");
/// Client-side install / uninstall scripts. The server serves these from
/// GET /install and GET /uninstall after replacing the `{SERVER_URL}`
/// placeholder with the URL the teammate just curl'd from, so the
/// resulting bash script already knows where to point the hook wrapper.
/// See scripts/runai-client-install.sh for the full doc.
const CLIENT_INSTALL_SH: &str = include_str!("../scripts/runai-client-install.sh");
const CLIENT_UNINSTALL_SH: &str = include_str!("../scripts/runai-client-uninstall.sh");

/// Shared state for handlers. Holds only the DB path (and AppPaths if needed
/// later for other resources) — rusqlite `Connection` is `!Sync`, so each
/// handler opens its own connection per request. SQLite open is cheap
/// (microseconds for the same file in the OS page cache); this keeps the
/// server lock-free and avoids serialising readers on a Mutex.
struct AppState {
    db_path: PathBuf,
}

impl AppState {
    fn db(&self) -> Result<Database> {
        Database::open(&self.db_path)
    }
}

/// Result of `ensure_running`. `AlreadyRunning` is the hot path for repeat
/// invocations (hook / SessionStart); `Started` happens once per machine boot.
#[derive(Debug, PartialEq, Eq)]
pub enum EnsureStatus {
    AlreadyRunning,
    Started,
}

/// Idempotent "is the dashboard up? if not, spawn it" helper. Designed to be
/// called from Claude Code's SessionStart hook (or any shell rc) so the
/// dashboard is always reachable without the user remembering to start it.
///
/// Behavior:
/// - If we can TCP-connect to `host:port` within 200ms → return `AlreadyRunning`.
///   This is the steady-state hot path (< 50ms total).
/// - Otherwise spawn `runai server --port P --host H` as a detached child with
///   stdio nullified, then poll the port for up to ~2s and return `Started`
///   when it comes up. Returns an error only if the spawn itself fails or the
///   server never binds.
///
/// The detached child becomes an orphan when this process exits and is
/// reparented to init (PID 1), which keeps the server running across the
/// lifetime of the launching shell / Claude Code session.
pub fn ensure_running(host: &str, port: u16) -> Result<EnsureStatus> {
    use std::net::TcpStream;
    use std::time::Duration;

    let addr_str = format!("{host}:{port}");
    let sock: SocketAddr = addr_str
        .parse()
        .with_context(|| format!("parse {addr_str}"))?;
    if TcpStream::connect_timeout(&sock, Duration::from_millis(200)).is_ok() {
        return Ok(EnsureStatus::AlreadyRunning);
    }

    let exe = std::env::current_exe().context("locate runai binary via current_exe")?;
    std::process::Command::new(&exe)
        .arg("server")
        .arg("--port")
        .arg(port.to_string())
        .arg("--host")
        .arg(host)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .with_context(|| format!("spawn `{}` server daemon", exe.display()))?;

    for _ in 0..40 {
        std::thread::sleep(Duration::from_millis(50));
        if TcpStream::connect_timeout(&sock, Duration::from_millis(100)).is_ok() {
            return Ok(EnsureStatus::Started);
        }
    }
    bail!("started runai server daemon but {addr_str} did not respond within 2s")
}

pub async fn serve(host: &str, port: u16) -> Result<()> {
    let paths = AppPaths::default_path();
    let state = Arc::new(AppState {
        db_path: paths.db_path(),
    });

    let app = Router::new()
        .route("/", get(serve_index))
        .route("/app.js", get(serve_app_js))
        .route("/app.css", get(serve_app_css))
        .route("/api/summary", get(api_summary))
        .route("/api/timeline", get(api_timeline))
        .route("/api/events", get(api_events))
        .route("/api/event/{id}", get(api_event_by_id))
        .route("/api/skills", get(api_skills))
        .route("/api/skill/{name}", get(api_skill_detail))
        .route("/api/skill/{name}/files", get(api_skill_files))
        .route("/api/skill/{name}/file", get(api_skill_file))
        // Remote-hook protocol: teammates' Claude Code UserPromptSubmit hooks
        // POST their standard hook JSON here and pipe stdout back into the
        // agent. See scripts/runai-client-install.sh for the wrapper they run.
        .route("/recommend", post(handle_recommend))
        .route("/skills/get/{name}", post(handle_skill_get))
        .route("/feedback", post(handle_feedback))
        .route("/install", get(handle_install_script))
        .route("/uninstall", get(handle_uninstall_script))
        .with_state(state);

    let addr: SocketAddr = format!("{host}:{port}")
        .parse()
        .with_context(|| format!("parse {host}:{port}"))?;
    println!("runai dashboard at http://{addr}");
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("bind {addr}"))?;
    axum::serve(listener, app).await.context("axum::serve")?;
    Ok(())
}

/// Process-lifetime cache-buster. Generated once when the server boots from
/// the current unix timestamp; injected into every `<link href="...">` /
/// `<script src="...">` URL in `index.html`. Every `runai server` restart
/// produces a fresh value, so the browser sees a different URL for the CSS
/// and JS and is forced to fetch the new bytes — no Cmd+Shift+R needed even
/// the first time after upgrade.
static BUILD_ID: std::sync::OnceLock<String> = std::sync::OnceLock::new();
fn build_id() -> &'static str {
    BUILD_ID.get_or_init(|| chrono::Utc::now().timestamp().to_string())
}

async fn serve_index() -> Response {
    // Rewrite static asset URLs to include the per-boot build_id query
    // string so cached entries from a prior server boot can never satisfy
    // a request for this boot's assets.
    let bid = build_id();
    let patched = INDEX_HTML
        .replace("\"/app.css\"", &format!("\"/app.css?v={bid}\""))
        .replace("\"/app.js\"", &format!("\"/app.js?v={bid}\""));
    dynamic_response(patched, "text/html; charset=utf-8")
}
async fn serve_app_js() -> Response {
    static_response(APP_JS, "application/javascript; charset=utf-8")
}
async fn serve_app_css() -> Response {
    static_response(APP_CSS, "text/css; charset=utf-8")
}

fn static_response(body: &'static str, content_type: &'static str) -> Response {
    // `no-store` + must-revalidate: assets are bundled into the binary via
    // `include_str!` so the only way they change is when the binary is
    // rebuilt. Cache-Control = no-store stops the browser from reading its
    // disk cache without revalidating; the cache-busting query string in
    // `serve_index` is the belt-and-braces defense that handles browsers
    // that ignored a no-store directive on prior responses.
    (
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, "no-store, must-revalidate"),
        ],
        body.to_string(),
    )
        .into_response()
}

fn dynamic_response(body: String, content_type: &'static str) -> Response {
    (
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, "no-store, must-revalidate"),
        ],
        body,
    )
        .into_response()
}

#[derive(Deserialize)]
struct EventsQuery {
    /// Filter to events newer than `now - hours` hours. None = all-time.
    hours: Option<i64>,
    /// Page size, default 50, hard-capped at 500.
    limit: Option<usize>,
    /// Zero-based offset.
    offset: Option<usize>,
    /// Filter by exact model name.
    model: Option<String>,
    /// Only return events where chosen != [].
    hit_only: Option<bool>,
}

#[derive(Serialize)]
struct PerModel {
    model: String,
    calls: i64,
    total_tokens: i64,
}

#[derive(Serialize)]
struct SummaryResponse {
    total: i64,
    hits: i64,
    errors: i64,
    hit_rate: f64,
    avg_latency_ms: Option<f64>,
    avg_prompt_tokens: f64,
    total_tokens: i64,
    per_model: Vec<PerModel>,
}

async fn api_summary(
    State(state): State<Arc<AppState>>,
    Query(q): Query<EventsQuery>,
) -> Result<Json<SummaryResponse>, ApiError> {
    let since = q.hours.map(hours_to_since_ts);
    let db = state.db()?;
    let stats = db.router_stats_summary(since)?;
    // Compute hit count separately — router_stats_summary doesn't have it.
    let total_with_hit = db.router_events_count(since, None, true)?;
    let avg_prompt = if stats.total_calls > 0 {
        stats.total_prompt_tokens as f64 / stats.total_calls as f64
    } else {
        0.0
    };
    let hit_rate = if stats.total_calls > 0 {
        total_with_hit as f64 / stats.total_calls as f64
    } else {
        0.0
    };
    Ok(Json(SummaryResponse {
        total: stats.total_calls,
        hits: total_with_hit,
        errors: stats.errors,
        hit_rate,
        avg_latency_ms: stats.avg_latency_ms,
        avg_prompt_tokens: avg_prompt,
        total_tokens: stats.total_tokens,
        per_model: stats
            .per_model
            .into_iter()
            .map(|m| PerModel {
                model: m.model,
                calls: m.calls,
                total_tokens: m.total_tokens,
            })
            .collect(),
    }))
}

#[derive(Serialize)]
struct EventsResponse {
    total: i64,
    events: Vec<EventJson>,
}

#[derive(Serialize)]
struct EventJson {
    id: Option<i64>,
    ts: i64,
    model: String,
    provider: String,
    status: String,
    mode: String,
    chosen: Vec<String>,
    candidate_count: i64,
    bm25_kept: i64,
    prompt_tokens: i64,
    completion_tokens: i64,
    total_tokens: i64,
    latency_ms: i64,
    session_id: String,
    user_prompt: String,
    cwd: String,
    error_msg: Option<String>,
    /// Raw LLM response (mode tag + skill names). Empty for legacy rows.
    llm_raw_response: String,
    /// Markdown block runai injected into Claude Code via hook stdout.
    /// Empty when chosen was empty or for legacy rows.
    hook_output: String,
    /// Full user message sent to the router LLM (history + already_routed +
    /// candidate listing + user prompt). Empty for pre-schema-v13 rows.
    llm_input: String,
    /// Whether the hook actually delivered a non-empty injection. Equivalent
    /// to `chosen` non-empty + status ok, exposed as a flat boolean for the UI.
    injected: bool,
}

impl From<RouterEvent> for EventJson {
    fn from(e: RouterEvent) -> Self {
        let chosen: Vec<String> = serde_json::from_str(&e.chosen_skills_json).unwrap_or_default();
        let injected = e.status == "ok" && !chosen.is_empty();
        EventJson {
            id: e.id,
            ts: e.ts,
            model: e.model,
            provider: e.provider,
            status: e.status,
            mode: e.mode,
            chosen,
            candidate_count: e.candidate_count,
            bm25_kept: e.bm25_kept,
            prompt_tokens: e.prompt_tokens,
            completion_tokens: e.completion_tokens,
            total_tokens: e.total_tokens,
            latency_ms: e.latency_ms,
            session_id: e.session_id,
            user_prompt: e.user_prompt,
            cwd: e.cwd,
            error_msg: e.error_msg,
            llm_raw_response: e.llm_raw_response,
            hook_output: e.hook_output,
            llm_input: e.llm_input,
            injected,
        }
    }
}

async fn api_events(
    State(state): State<Arc<AppState>>,
    Query(q): Query<EventsQuery>,
) -> Result<Json<EventsResponse>, ApiError> {
    let since = q.hours.map(hours_to_since_ts);
    let limit = q.limit.unwrap_or(50).min(500);
    let offset = q.offset.unwrap_or(0);
    let model_ref = q.model.as_deref();
    let hit_only = q.hit_only.unwrap_or(false);
    let db = state.db()?;
    let events = db.router_events_paged(since, limit, offset, model_ref, hit_only)?;
    let total = db.router_events_count(since, model_ref, hit_only)?;
    Ok(Json(EventsResponse {
        total,
        events: events.into_iter().map(EventJson::from).collect(),
    }))
}

#[derive(Deserialize)]
struct TimelineQuery {
    /// Window length in hours. 24 -> 24 hourly buckets; 6 -> 6 hourly buckets.
    hours: Option<i64>,
    /// Optional bucket width override in seconds. Default = hours * 3600 / 24
    /// (so 24h -> hourly, 6h -> 15min, etc), capped to keep the chart legible.
    bucket_secs: Option<i64>,
}

#[derive(Serialize)]
struct TimelinePoint {
    ts_start: i64,
    total: i64,
    hits: i64,
    errors: i64,
    avg_latency_ms: f64,
}

#[derive(Serialize)]
struct TimelineResponse {
    bucket_secs: i64,
    points: Vec<TimelinePoint>,
}

async fn api_timeline(
    State(state): State<Arc<AppState>>,
    Query(q): Query<TimelineQuery>,
) -> Result<Json<TimelineResponse>, ApiError> {
    let hours = q.hours.unwrap_or(24).clamp(1, 720);
    let target_buckets = 48i64;
    let default_bucket = ((hours * 3600) / target_buckets).max(60);
    let bucket_secs = q.bucket_secs.unwrap_or(default_bucket).max(60);
    let buckets = ((hours * 3600) / bucket_secs).max(1);
    let db = state.db()?;
    let raw = db.router_timeline(bucket_secs, buckets)?;
    Ok(Json(TimelineResponse {
        bucket_secs,
        points: raw
            .into_iter()
            .map(|b| TimelinePoint {
                ts_start: b.ts_start,
                total: b.total,
                hits: b.hits,
                errors: b.errors,
                avg_latency_ms: b.avg_latency_ms,
            })
            .collect(),
    }))
}

#[derive(Serialize)]
struct SkillRow {
    name: String,
    description: String,
    usage_count: i64,
    summary: String,
    llm_score: Option<i64>,
}

#[derive(Serialize)]
struct SkillsResponse {
    total: usize,
    enriched: usize,
    skills: Vec<SkillRow>,
}

async fn api_skills(State(state): State<Arc<AppState>>) -> Result<Json<SkillsResponse>, ApiError> {
    use crate::core::manager::SkillManager;
    use crate::core::resource::ResourceKind;

    let mgr = SkillManager::with_base(state.db_path.parent().unwrap().to_path_buf())
        .map_err(ApiError::Internal)?;
    let resources = mgr.list_resources(None, None).map_err(ApiError::Internal)?;
    let db = state.db()?;
    let summaries = db.skill_ai_summary_all().unwrap_or_default();
    let scores = db.skill_llm_scores_all().unwrap_or_default();

    let mut skills = Vec::new();
    let mut enriched = 0usize;
    for r in resources {
        if r.kind != ResourceKind::Skill {
            continue;
        }
        let summary = summaries.get(&r.name).cloned().unwrap_or_default();
        if !summary.is_empty() {
            enriched += 1;
        }
        let llm_score = scores.get(&r.name).copied();
        skills.push(SkillRow {
            name: r.name.clone(),
            description: r.description.clone(),
            usage_count: r.usage_count as i64,
            summary,
            llm_score,
        });
    }
    let total = skills.len();
    skills.sort_by(|a, b| {
        b.llm_score
            .unwrap_or(-1)
            .cmp(&a.llm_score.unwrap_or(-1))
            .then(a.name.cmp(&b.name))
    });
    Ok(Json(SkillsResponse {
        total,
        enriched,
        skills,
    }))
}

#[derive(Serialize)]
struct SkillDetailResponse {
    name: String,
    description: String,
    usage_count: i64,
    summary: String,
    llm_score: Option<i64>,
    skill_md_path: String,
    skill_md_content: String,
    skill_md_size: usize,
    skill_md_truncated: bool,
    /// router_events where this skill was chosen, newest first, up to 50.
    events: Vec<EventJson>,
    events_total: usize,
}

async fn api_skill_detail(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<SkillDetailResponse>, ApiError> {
    use crate::core::manager::SkillManager;
    use crate::core::resource::ResourceKind;

    let mgr = SkillManager::with_base(state.db_path.parent().unwrap().to_path_buf())
        .map_err(ApiError::Internal)?;
    let resources = mgr.list_resources(None, None).map_err(ApiError::Internal)?;
    let resource = resources
        .into_iter()
        .find(|r| r.kind == ResourceKind::Skill && r.name == name)
        .ok_or(ApiError::NotFound)?;
    let db = state.db()?;
    let summary = db.skill_ai_summary(&name).unwrap_or_default();
    let llm_score = if summary.is_empty() {
        None
    } else {
        Some(db.skill_llm_score(&name).unwrap_or(5))
    };
    let skill_md_path = mgr.paths().skills_dir().join(&name).join("SKILL.md");
    const MAX_BYTES: usize = 60_000;
    let (skill_md_content, truncated, total_size) = match std::fs::read_to_string(&skill_md_path) {
        Ok(body) => {
            let total = body.len();
            if total > MAX_BYTES {
                let trunc: String = body.chars().take(MAX_BYTES).collect();
                (trunc, true, total)
            } else {
                (body, false, total)
            }
        }
        Err(_) => (String::new(), false, 0),
    };
    let event_rows = db.router_events_for_skill(&name, 50).unwrap_or_default();
    let events_total = event_rows.len();
    let events: Vec<EventJson> = event_rows.into_iter().map(EventJson::from).collect();
    Ok(Json(SkillDetailResponse {
        name: resource.name.clone(),
        description: resource.description.clone(),
        usage_count: resource.usage_count as i64,
        summary,
        llm_score,
        skill_md_path: skill_md_path.display().to_string(),
        skill_md_content,
        skill_md_size: total_size,
        skill_md_truncated: truncated,
        events,
        events_total,
    }))
}

#[derive(Serialize)]
struct SkillFileEntry {
    /// Path relative to the skill directory (forward slashes).
    path: String,
    size: u64,
    is_text: bool,
}

#[derive(Serialize)]
struct SkillFilesResponse {
    name: String,
    skill_dir: String,
    entries: Vec<SkillFileEntry>,
}

#[derive(Serialize)]
struct SkillFileResponse {
    path: String,
    size: u64,
    /// File contents. Empty for binaries; binary files only return metadata.
    content: String,
    /// True if the file content was cut off due to size cap.
    truncated: bool,
    /// True if we returned content. False for binary/unsupported types —
    /// `content` will be empty and the UI should display a placeholder.
    is_text: bool,
}

#[derive(Deserialize)]
struct SkillFileQuery {
    path: String,
}

async fn api_skill_files(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<SkillFilesResponse>, ApiError> {
    use crate::core::manager::SkillManager;
    let mgr = SkillManager::with_base(state.db_path.parent().unwrap().to_path_buf())
        .map_err(ApiError::Internal)?;
    let skill_dir = mgr.paths().skills_dir().join(&name);
    if !skill_dir.is_dir() {
        return Err(ApiError::NotFound);
    }
    let mut entries: Vec<SkillFileEntry> = Vec::new();
    walk_skill_dir(&skill_dir, &skill_dir, &mut entries)?;
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(Json(SkillFilesResponse {
        name,
        skill_dir: skill_dir.display().to_string(),
        entries,
    }))
}

fn walk_skill_dir(
    root: &std::path::Path,
    dir: &std::path::Path,
    out: &mut Vec<SkillFileEntry>,
) -> Result<(), ApiError> {
    let read = std::fs::read_dir(dir).map_err(|e| ApiError::Internal(e.into()))?;
    for entry in read {
        let entry = entry.map_err(|e| ApiError::Internal(e.into()))?;
        let path = entry.path();
        let file_name = entry.file_name();
        let fname_str = file_name.to_string_lossy();
        // Skip hidden/junk
        if fname_str.starts_with('.') {
            continue;
        }
        let md = entry.metadata().map_err(|e| ApiError::Internal(e.into()))?;
        if md.is_dir() {
            walk_skill_dir(root, &path, out)?;
        } else if md.is_file() {
            let rel = path
                .strip_prefix(root)
                .map_err(|e| ApiError::Internal(anyhow::anyhow!("strip_prefix: {e}")))?
                .to_string_lossy()
                .replace('\\', "/");
            out.push(SkillFileEntry {
                path: rel,
                size: md.len(),
                is_text: is_text_path(&path),
            });
        }
    }
    Ok(())
}

fn is_text_path(p: &std::path::Path) -> bool {
    let ext = p
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(
        ext.as_str(),
        "md" | "markdown"
            | "txt"
            | "json"
            | "yaml"
            | "yml"
            | "toml"
            | "ini"
            | "sh"
            | "bash"
            | "zsh"
            | "fish"
            | "py"
            | "js"
            | "ts"
            | "tsx"
            | "jsx"
            | "mjs"
            | "cjs"
            | "rs"
            | "go"
            | "java"
            | "c"
            | "cc"
            | "cpp"
            | "h"
            | "hpp"
            | "css"
            | "scss"
            | "html"
            | "xml"
            | "xsd"
            | "xsl"
            | "xslt"
            | "dtd"
            | "csv"
            | "tsv"
            | "log"
            | "vue"
            | "svelte"
            | "rb"
            | "php"
            | "lua"
            | "swift"
            | "kt"
            | "kts"
            | "rst"
            | "tex"
            | "sql"
            | "dockerfile"
            | "makefile"
            | "env"
            | ""
    )
}

async fn api_skill_file(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Query(q): Query<SkillFileQuery>,
) -> Result<Json<SkillFileResponse>, ApiError> {
    use crate::core::manager::SkillManager;
    let mgr = SkillManager::with_base(state.db_path.parent().unwrap().to_path_buf())
        .map_err(ApiError::Internal)?;
    let skill_dir = mgr.paths().skills_dir().join(&name);
    let target = skill_dir.join(&q.path);
    // SECURITY: canonicalise both, verify target still under skill_dir.
    // Prevents `?path=../../etc/passwd` style traversal.
    let root_real = skill_dir
        .canonicalize()
        .map_err(|e| ApiError::Internal(e.into()))?;
    let target_real = match target.canonicalize() {
        Ok(p) => p,
        Err(_) => return Err(ApiError::NotFound),
    };
    if !target_real.starts_with(&root_real) {
        return Err(ApiError::NotFound);
    }
    let md = target_real.metadata().map_err(|_| ApiError::NotFound)?;
    if md.is_dir() {
        return Err(ApiError::NotFound);
    }
    let size = md.len();
    let is_text = is_text_path(&target_real);
    const MAX_BYTES: usize = 80_000;
    let (content, truncated) = if is_text {
        match std::fs::read_to_string(&target_real) {
            Ok(s) => {
                if s.len() > MAX_BYTES {
                    (s.chars().take(MAX_BYTES).collect::<String>(), true)
                } else {
                    (s, false)
                }
            }
            // text by extension but not valid UTF-8 → treat as binary
            Err(_) => {
                return Ok(Json(SkillFileResponse {
                    path: q.path,
                    size,
                    content: String::new(),
                    truncated: false,
                    is_text: false,
                }));
            }
        }
    } else {
        (String::new(), false)
    };
    Ok(Json(SkillFileResponse {
        path: q.path,
        size,
        content,
        truncated,
        is_text,
    }))
}

/// Pull a single field from the Claude Code hook payload, defaulting to
/// empty string when missing.
fn payload_str(payload: &serde_json::Value, key: &str) -> String {
    payload
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

/// POST /recommend — runai's remote skill router.
///
/// Body: the standard Claude Code UserPromptSubmit hook JSON (fields used:
/// `prompt`, `session_id`, `cwd`, `transcript_path`).
///
/// Optional `X-Runai-User: {user}@{host}` header — when present, the
/// teammate's identity is prefixed into the `session_id` so multiple
/// teammates' sessions don't collide in the router's per-session memory.
/// The install script writes this header automatically; manual callers can
/// omit it.
///
/// Returns the hook output string (markdown to be injected into the
/// teammate's Claude Code prompt) as plain text. Errors fall through to
/// 200 + empty body — the install script's `--max-time 30 || true`
/// pattern means a server hiccup never blocks the teammate's prompt.
async fn handle_recommend(headers: HeaderMap, Json(payload): Json<serde_json::Value>) -> Response {
    let user_prefix = headers
        .get("X-Runai-User")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("")
        .to_string();
    // Server-rendered hook output points the agent at THIS server. URL
    // derived from the request's Host header; falls back to the
    // dashboard default when missing. User header gets pasted into every
    // curl call so the server can session-prefix per teammate.
    let server_url = guess_server_url(&headers);
    let user_header_arg = if user_prefix.is_empty() {
        String::new()
    } else {
        format!(" -H 'X-Runai-User: {user_prefix}'")
    };

    // recommend() is blocking (reqwest::blocking + rusqlite). Hop onto a
    // blocking thread so the async runtime stays responsive.
    let join = tokio::task::spawn_blocking(move || -> Result<String> {
        let mgr = SkillManager::new()?;
        let prompt = payload_str(&payload, "prompt");
        if prompt.is_empty() {
            return Ok(String::new());
        }
        let cwd = payload_str(&payload, "cwd");
        let transcript = payload_str(&payload, "transcript_path");
        let claude_sid = payload_str(&payload, "session_id");

        // session_id is `{user_prefix}:{claude_sid}` when both present;
        // either alone when only one; empty when neither (single-user
        // local-test path).
        let sid_string: String = match (user_prefix.is_empty(), claude_sid.is_empty()) {
            (false, false) => format!("{user_prefix}:{claude_sid}"),
            (false, true) => user_prefix.clone(),
            (true, false) => claude_sid.clone(),
            (true, true) => String::new(),
        };

        let tpath_pb = if transcript.is_empty() {
            None
        } else {
            Some(std::path::PathBuf::from(&transcript))
        };
        let sid_opt = if sid_string.is_empty() {
            None
        } else {
            Some(sid_string.as_str())
        };
        let cwd_opt = if cwd.is_empty() {
            None
        } else {
            Some(cwd.as_str())
        };

        let decision = recommend::recommend(&mgr, &prompt, tpath_pb.as_deref(), sid_opt, cwd_opt)?;

        let history = match sid_opt {
            Some(s) if !s.is_empty() => mgr
                .db()
                .router_session_recommended_skills(s)
                .unwrap_or_default(),
            _ => Vec::new(),
        };

        Ok(recommend::format_for_hook_full(
            &decision,
            sid_opt.unwrap_or(""),
            &history,
            &server_url,
            &user_header_arg,
        ))
    })
    .await;

    let body = match join {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            eprintln!("/recommend: recommend() failed: {e:#}");
            String::new()
        }
        Err(e) => {
            eprintln!("/recommend: spawn_blocking join failed: {e}");
            String::new()
        }
    };
    ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], body).into_response()
}

/// GET /install — return the client install bash script with `{SERVER_URL}`
/// substituted by the URL the request came in on. Teammate runs:
///   curl -fsSL http://<server>:<port>/install | bash
/// Query string for /skills/get/{name}: optional `session_id` used to
/// session-prefix the adoption row.
#[derive(Deserialize)]
struct SkillGetQuery {
    #[serde(default)]
    session_id: String,
}

/// POST /skills/get/{name} — return SKILL.md body + record adoption.
///
/// Replaces the local-only `runai recommend get <name>` command for users
/// who don't have the binary. Side-effects (idempotent):
///   - record_usage: bumps the skill's usage_count
///   - record_session_adoption: writes (session_id, skill_name) row
///
/// session id = `{X-Runai-User}:{session_id query}` when both present.
async fn handle_skill_get(
    headers: HeaderMap,
    Path(name): Path<String>,
    Query(q): Query<SkillGetQuery>,
) -> Response {
    let user_prefix = headers
        .get("X-Runai-User")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("")
        .to_string();
    let claude_sid = q.session_id;

    let join = tokio::task::spawn_blocking(move || -> Result<String> {
        let mgr = SkillManager::new()?;
        let skill_md = mgr.paths().skills_dir().join(&name).join("SKILL.md");
        let content = std::fs::read_to_string(&skill_md)
            .with_context(|| format!("read {}", skill_md.display()))?;

        let _ = mgr.record_usage(&name);
        let sid_string = match (user_prefix.is_empty(), claude_sid.is_empty()) {
            (false, false) => format!("{user_prefix}:{claude_sid}"),
            (false, true) => user_prefix.clone(),
            (true, false) => claude_sid.clone(),
            (true, true) => String::new(),
        };
        if !sid_string.is_empty() {
            let _ = mgr.db().record_session_adoption(&sid_string, &name);
        }
        Ok(content)
    })
    .await;

    match join {
        Ok(Ok(content)) => (
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            content,
        )
            .into_response(),
        Ok(Err(e)) => {
            eprintln!("/skills/get: {e:#}");
            (
                StatusCode::NOT_FOUND,
                [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
                format!("skill not found: {e}\n"),
            )
                .into_response()
        }
        Err(e) => {
            eprintln!("/skills/get: spawn_blocking join failed: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
                String::from("internal error\n"),
            )
                .into_response()
        }
    }
}

#[derive(Deserialize)]
struct FeedbackBody {
    skill: String,
    note: String,
}

/// POST /feedback — replaces `runai recommend feedback`.
/// Body: `{"skill":"...","note":"..."}`.
async fn handle_feedback(headers: HeaderMap, Json(req): Json<FeedbackBody>) -> Response {
    let user_prefix = headers
        .get("X-Runai-User")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("")
        .to_string();

    let join = tokio::task::spawn_blocking(move || -> Result<String> {
        let mgr = SkillManager::new()?;
        let report = recommend::reevaluate_skill(&mgr, &req.skill, &req.note)?;
        Ok(format!(
            "feedback applied by {user_prefix}: {} llm_score {} → {} (summary {} chars)\n",
            req.skill, report.old_score, report.new_score, report.new_summary_len
        ))
    })
    .await;

    match join {
        Ok(Ok(s)) => ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], s).into_response(),
        Ok(Err(e)) => {
            eprintln!("/feedback: {e:#}");
            (
                StatusCode::BAD_REQUEST,
                [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
                format!("feedback error: {e}\n"),
            )
                .into_response()
        }
        Err(e) => {
            eprintln!("/feedback: spawn_blocking join failed: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
                String::from("internal error\n"),
            )
                .into_response()
        }
    }
}

async fn handle_install_script(headers: HeaderMap) -> Response {
    let server_url = guess_server_url(&headers);
    let body = CLIENT_INSTALL_SH.replace("{SERVER_URL}", &server_url);
    (
        [(header::CONTENT_TYPE, "text/x-shellscript; charset=utf-8")],
        body,
    )
        .into_response()
}

/// GET /uninstall — return the client uninstall bash script. Reverses
/// /install: removes the hook entry from Claude Code settings.json and
/// deletes ~/.runai-hook.sh.
async fn handle_uninstall_script() -> Response {
    (
        [(header::CONTENT_TYPE, "text/x-shellscript; charset=utf-8")],
        CLIENT_UNINSTALL_SH.to_string(),
    )
        .into_response()
}

/// Reconstruct the URL the teammate curl'd from so the install script ends
/// up hard-coded with the same `http://host:port` they used. Falls back to
/// the `Host` header (curl always sets this); scheme defaults to `http`
/// since there's no TLS in front of runai by default — LAN deploys.
fn guess_server_url(headers: &HeaderMap) -> String {
    let host = headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("127.0.0.1:17888");
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("http");
    format!("{scheme}://{host}")
}

async fn api_event_by_id(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<EventJson>, ApiError> {
    let db = state.db()?;
    match db.router_event_by_id(id)? {
        Some(ev) => Ok(Json(ev.into())),
        None => Err(ApiError::NotFound),
    }
}

fn hours_to_since_ts(hours: i64) -> i64 {
    let now = chrono::Utc::now().timestamp();
    now - hours.max(0) * 3600
}

/// API error wrapper that maps anyhow into proper HTTP responses.
enum ApiError {
    Internal(anyhow::Error),
    NotFound,
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        ApiError::Internal(e)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match self {
            ApiError::Internal(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response(),
            ApiError::NotFound => (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "not found"})),
            )
                .into_response(),
        }
    }
}
