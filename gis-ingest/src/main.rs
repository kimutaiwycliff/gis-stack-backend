mod error;
mod inspect;
mod jobs;
mod load;
mod validate;

use axum::{
    extract::{Multipart, Path, State},
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        Html, IntoResponse, Response,
    },
    routing::{get, post},
    Json, Router,
};
use error::Result;
use jobs::{JobStage, JobStore};
use serde::{Deserialize, Serialize};
use std::{convert::Infallible, sync::Arc, time::Duration};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
use tower_http::cors::CorsLayer;
use uuid::Uuid;

#[derive(Clone)]
struct AppState {
    db_url: String,
    jobs: JobStore,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let db_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set");
    let port: u16 = std::env::var("INGEST_PORT")
        .unwrap_or_else(|_| "8000".into())
        .parse()
        .expect("INGEST_PORT must be a number");

    let jobs = jobs::new_store();
    jobs::spawn_cleanup(Arc::clone(&jobs));

    let state = AppState { db_url, jobs };

    let static_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.join("static")))
        .unwrap_or_else(|| std::path::PathBuf::from("static"));

    let app = Router::new()
        .nest(
            "/ingest",
            Router::new()
                .route("/", get(ui_handler))
                .route("/health", get(health_handler))
                .route("/api/inspect", post(inspect_handler))
                .route("/api/load", post(load_handler))
                .route("/api/jobs/:id/events", get(job_events_handler))
                .route("/api/jobs/:id", get(job_status_handler))
                .nest_service(
                    "/static",
                    tower_http::services::ServeDir::new(static_dir),
                ),
        )
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("gis-ingest listening on http://{}/ingest/", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

// ── Handlers ──────────────────────────────────────────────────

async fn health_handler() -> impl IntoResponse {
    "ok"
}

async fn ui_handler() -> impl IntoResponse {
    let html = include_str!("../static/index.html");
    Html(html)
}

// ── POST /ingest/api/inspect ──────────────────────────────────

#[derive(Deserialize)]
#[serde(untagged)]
enum InspectInput {
    Url { url: String },
    // File upload handled via multipart below
}

async fn inspect_handler(
    State(_state): State<AppState>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse> {
    let tmp = tempfile::Builder::new()
        .prefix("gis-ingest-")
        .tempdir()
        .map_err(|e| error::AppError(e.into()))?;

    let mut file_path: Option<std::path::PathBuf> = None;
    let mut url: Option<String> = None;

    // Try to read a field named "file" or "url"
    while let Some(field) = multipart.next_field().await.map_err(|e| error::AppError(e.into()))? {
        let name = field.name().unwrap_or("").to_string();
        if name == "url" {
            url = Some(field.text().await.map_err(|e| error::AppError(e.into()))?);
            break;
        }
        if name == "file" {
            let filename = field
                .file_name()
                .unwrap_or("upload.bin")
                .to_string();
            let dest = tmp.path().join(&filename);
            let data = field.bytes().await.map_err(|e| error::AppError(e.into()))?;
            tokio::fs::write(&dest, &data).await.map_err(|e| error::AppError(e.into()))?;
            file_path = Some(dest);
            break;
        }
    }

    // keep() prevents auto-deletion when this handler returns so that the load
    // endpoint can still find the file.  The directory persists until the OS cleans
    // up /tmp (acceptable for a dev/internal tool).
    let inspect_path = if let Some(u) = url {
        let ext = inspect::url_extension(&u);
        let dest = tmp.path().join(format!("download.{}", ext));
        inspect::download_url(&u, &dest)
            .await
            .map_err(|e| error::AppError(e))?;
        let _ = tmp.keep(); // prevent auto-deletion when this handler returns
        dest
    } else if let Some(fp) = file_path {
        let _ = tmp.keep(); // prevent auto-deletion when this handler returns
        fp
    } else {
        return Err(error::AppError(anyhow::anyhow!(
            "No file or URL provided. Send a multipart field named 'file' or 'url'."
        )));
    };

    let result = inspect::inspect_file(&inspect_path)
        .await
        .map_err(|e| error::AppError(e))?;

    // Store the temp path in the response so the load endpoint can use it
    // We encode as a job-less "pending source" by returning the temp path.
    // For simplicity, we persist the temp dir by leaking it — cleaned by jobs GC.
    // In production, associate with a session/token.
    let source_path = inspect_path.to_string_lossy().to_string();

    Ok(Json(serde_json::json!({
        "source_path": source_path,
        "inspect": result,
    })))
}

// ── POST /ingest/api/load ─────────────────────────────────────

#[derive(Deserialize)]
struct LoadBody {
    source_path: String,
    schema: String,
    table: String,
    mode: load::LoadMode,
    layer_name: Option<String>,
}

#[derive(Serialize)]
struct LoadResponse {
    job_id: Uuid,
}

async fn load_handler(
    State(state): State<AppState>,
    Json(body): Json<LoadBody>,
) -> Result<impl IntoResponse> {
    // Basic validation
    if body.table.is_empty() {
        return Err(error::AppError(anyhow::anyhow!("table name is required")));
    }
    if !matches!(body.schema.as_str(), "gis" | "staging" | "raster" | "public") {
        return Err(error::AppError(anyhow::anyhow!(
            "schema must be one of: gis, staging, raster, public"
        )));
    }

    let (job_id, _rx) = jobs::create_job(&state.jobs);

    let req = load::LoadRequest {
        source_path: std::path::PathBuf::from(&body.source_path),
        schema: body.schema,
        table: body.table,
        mode: body.mode,
        layer_name: body.layer_name,
    };

    let db_url = state.db_url.clone();
    let store = Arc::clone(&state.jobs);

    tokio::spawn(async move {
        load::run_load_pipeline(req, db_url, job_id, store).await;
    });

    Ok(Json(LoadResponse { job_id }))
}

// ── GET /ingest/api/jobs/:id/events (SSE) ─────────────────────

async fn job_events_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Response {
    // Snapshot historical events AND subscribe to the live channel atomically
    // so we never miss an event regardless of when the browser connects.
    let (past_events, rx) = {
        match state.jobs.get(&id) {
            Some(entry) => {
                let job = entry.value().lock().await;
                (job.events.clone(), job.tx.subscribe())
            }
            None => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({"error": "job not found"})),
                )
                    .into_response();
            }
        }
    };

    // Replay already-emitted events first, then stream live ones.
    let replay = tokio_stream::iter(past_events).map(|event| {
        let data = serde_json::to_string(&event).unwrap_or_default();
        Ok::<Event, Infallible>(Event::default().data(data))
    });

    let live = BroadcastStream::new(rx).filter_map(|msg| {
        let msg = msg.ok()?;
        let data = serde_json::to_string(&msg).ok()?;
        Some(Ok::<Event, Infallible>(Event::default().data(data)))
    });

    let stream = replay.chain(live);

    Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(10)))
        .into_response()
}

// ── GET /ingest/api/jobs/:id ──────────────────────────────────

#[derive(Serialize)]
struct JobStatusResponse {
    id: Uuid,
    stage: JobStage,
    log: Vec<String>,
}

async fn job_status_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse> {
    match state.jobs.get(&id) {
        Some(entry) => {
            let job = entry.lock().await;
            Ok(Json(JobStatusResponse {
                id: job.id,
                stage: job.stage.clone(),
                log: job.log.clone(),
            }))
        }
        None => Err(error::AppError(anyhow::anyhow!("job not found"))),
    }
}
