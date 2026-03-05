use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use std::process::Stdio;

use crate::jobs::{JobStage, JobStore, SseEvent};
use crate::validate;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum LoadMode {
    Overwrite,
    Append,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadRequest {
    pub source_path: PathBuf,
    pub schema: String,
    pub table: String,
    pub mode: LoadMode,
    pub layer_name: Option<String>,
}

/// Runs the full load pipeline in the background, emitting SSE events.
pub async fn run_load_pipeline(
    req: LoadRequest,
    db_url: String,
    job_id: Uuid,
    store: JobStore,
) {
    if let Err(e) = do_load(&req, &db_url, job_id, &store).await {
        crate::jobs::emit(
            &store,
            job_id,
            SseEvent {
                stage: JobStage::Failed,
                message: format!("Error: {}", e),
                data: None,
            },
        )
        .await;
    }
}

async fn do_load(
    req: &LoadRequest,
    db_url: &str,
    job_id: Uuid,
    store: &JobStore,
) -> Result<()> {
    // ── Build ogr2ogr PG connection string ──────────────────
    // Convert postgresql:// URL to "PG:host=... ..." format that ogr2ogr accepts
    let pg_dsn = format!("PG:{}", db_url_to_pg_dsn(db_url)?);
    let target = format!("{}.{}", req.schema, req.table);

    crate::jobs::emit(
        store,
        job_id,
        SseEvent {
            stage: JobStage::Loading,
            message: format!("Loading into {} …", target),
            data: None,
        },
    )
    .await;

    let mut cmd = Command::new("ogr2ogr");
    cmd.arg("-f").arg("PostgreSQL")
        .arg(&pg_dsn)
        .arg(req.source_path.as_os_str())
        .arg("-nln").arg(&target)
        .arg("-nlt").arg("PROMOTE_TO_MULTI")
        .arg("-lco").arg("GEOMETRY_NAME=geom")
        .arg("-lco").arg("FID=id")
        .arg("-lco").arg("SPATIAL_INDEX=GIST")
        .arg("-t_srs").arg("EPSG:4326")
        .arg("--config").arg("PG_USE_COPY").arg("YES")
        .arg("-progress");

    if let Some(layer) = &req.layer_name {
        cmd.arg(layer);
    }

    match req.mode {
        LoadMode::Overwrite => { cmd.arg("-overwrite"); }
        LoadMode::Append    => { cmd.arg("-append"); }
    }

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = cmd.spawn().context("failed to spawn ogr2ogr")?;

    // Get the broadcast sender so we can emit from both stdout and stderr tasks
    let tx = {
        if let Some(entry) = store.get(&job_id) {
            entry.lock().await.tx.clone()
        } else {
            bail!("job not found");
        }
    };

    // Stream stderr (ogr2ogr writes progress to stderr)
    let stderr = child.stderr.take().expect("stderr not captured");
    let tx_err = tx.clone();
    let stderr_task = tokio::spawn(async move {
        let mut reader = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            if !line.trim().is_empty() {
                let _ = tx_err.send(SseEvent {
                    stage: JobStage::Loading,
                    message: line,
                    data: None,
                });
            }
        }
    });

    // Stream stdout
    let stdout = child.stdout.take().expect("stdout not captured");
    let tx_out = tx.clone();
    let stdout_task = tokio::spawn(async move {
        let mut reader = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            if !line.trim().is_empty() {
                let _ = tx_out.send(SseEvent {
                    stage: JobStage::Loading,
                    message: line,
                    data: None,
                });
            }
        }
    });

    let status = child.wait().await.context("ogr2ogr process failed")?;
    let _ = tokio::join!(stderr_task, stdout_task);

    if !status.success() {
        bail!("ogr2ogr exited with status {}", status);
    }

    // ── Post-load validation ────────────────────────────────
    crate::jobs::emit(
        store,
        job_id,
        SseEvent {
            stage: JobStage::Validating,
            message: "Running PostGIS validation …".to_string(),
            data: None,
        },
    )
    .await;

    let result = validate::validate_table(db_url, &req.schema, &req.table).await?;

    crate::jobs::emit(
        store,
        job_id,
        SseEvent {
            stage: JobStage::Done,
            message: format!(
                "Done — {} rows loaded ({} null geom, {} invalid geom)",
                result.total_rows, result.null_geom, result.invalid_geom
            ),
            data: Some(serde_json::to_value(&result).unwrap_or_default()),
        },
    )
    .await;

    Ok(())
}

/// Convert postgresql://user:pass@host:port/db  →  "host=... port=... dbname=... user=... password=..."
fn db_url_to_pg_dsn(url: &str) -> Result<String> {
    let url = url::Url::parse(url).context("invalid DATABASE_URL")?;
    let host = url.host_str().unwrap_or("localhost");
    let port = url.port().unwrap_or(5432);
    let dbname = url.path().trim_start_matches('/');
    let user = url.username();
    let password = url.password().unwrap_or("");
    Ok(format!(
        "host={} port={} dbname={} user={} password={}",
        host, port, dbname, user, password
    ))
}
