use anyhow::{bail, Context, Result};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::io::AsyncWriteExt;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldInfo {
    pub name: String,
    #[serde(rename = "type")]
    pub field_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerInfo {
    pub name: String,
    pub feature_count: i64,
    pub geometry_type: String,
    pub crs: Option<String>,
    pub extent: Option<[f64; 4]>,
    pub fields: Vec<FieldInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectResult {
    pub format: String,
    pub layers: Vec<LayerInfo>,
}

/// Run `ogrinfo -al -so -json <path>` and parse the result.
pub async fn inspect_file(path: &Path) -> Result<InspectResult> {
    let output = tokio::process::Command::new("ogrinfo")
        .args(["-al", "-so", "-json", path.to_str().unwrap_or("")])
        .output()
        .await
        .context("ogrinfo not found — is GDAL installed?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("ogrinfo failed: {}", stderr.trim());
    }

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("failed to parse ogrinfo JSON output")?;

    parse_ogrinfo_json(&json)
}

fn parse_ogrinfo_json(v: &serde_json::Value) -> Result<InspectResult> {
    let driver = v
        .get("driverShortName")
        .and_then(|d| d.as_str())
        .unwrap_or("Unknown")
        .to_string();

    let layers_arr = v
        .get("layers")
        .and_then(|l| l.as_array())
        .cloned()
        .unwrap_or_default();

    let mut layers = Vec::new();
    for layer in &layers_arr {
        let name = layer
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("unnamed")
            .to_string();

        let feature_count = layer
            .get("featureCount")
            .and_then(|n| n.as_i64())
            .unwrap_or(-1);

        let geometry_type = layer
            .get("geometryFields")
            .and_then(|gf| gf.as_array())
            .and_then(|arr| arr.first())
            .and_then(|g| g.get("type"))
            .and_then(|t| t.as_str())
            .unwrap_or("Unknown")
            .to_string();

        // CRS from geometryFields[0].coordinateSystem.projjson.name or .wkt
        let crs = layer
            .get("geometryFields")
            .and_then(|gf| gf.as_array())
            .and_then(|arr| arr.first())
            .and_then(|g| g.get("coordinateSystem"))
            .and_then(|cs| {
                // Try authority code first (e.g. "EPSG:4326")
                let auth = cs.get("projjson")
                    .and_then(|pj| pj.get("id"))
                    .and_then(|id| {
                        let auth = id.get("authority").and_then(|a| a.as_str())?;
                        let code = id.get("code").and_then(|c| c.as_i64())?;
                        Some(format!("{}:{}", auth, code))
                    });
                auth.or_else(|| {
                    cs.get("wkt").and_then(|w| w.as_str()).map(|s| s.lines().next().unwrap_or(s).to_string())
                })
            });

        // Extent
        let extent = layer.get("extent").and_then(|e| {
            let minx = e.get("minx")?.as_f64()?;
            let miny = e.get("miny")?.as_f64()?;
            let maxx = e.get("maxx")?.as_f64()?;
            let maxy = e.get("maxy")?.as_f64()?;
            Some([minx, miny, maxx, maxy])
        });

        // Fields
        let fields = layer
            .get("fields")
            .and_then(|f| f.as_array())
            .map(|arr| {
                arr.iter()
                    .map(|f| FieldInfo {
                        name: f.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string(),
                        field_type: f.get("type").and_then(|t| t.as_str()).unwrap_or("String").to_string(),
                    })
                    .collect()
            })
            .unwrap_or_default();

        layers.push(LayerInfo {
            name,
            feature_count,
            geometry_type,
            crs,
            extent,
            fields,
        });
    }

    Ok(InspectResult {
        format: driver,
        layers,
    })
}

/// Stream-download a URL to `dest` path. Returns the final file size.
pub async fn download_url(url: &str, dest: &Path) -> Result<u64> {
    let client = reqwest::Client::builder()
        .user_agent("gis-ingest/0.1")
        .timeout(std::time::Duration::from_secs(300))
        .build()?;

    let resp = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("failed to fetch URL: {}", url))?;

    if !resp.status().is_success() {
        bail!("HTTP {} for URL: {}", resp.status(), url);
    }

    let mut file = tokio::fs::File::create(dest)
        .await
        .context("failed to create temp file for download")?;

    let mut stream = resp.bytes_stream();
    let mut total: u64 = 0;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("error reading response body")?;
        total += chunk.len() as u64;
        file.write_all(&chunk).await.context("failed to write download chunk")?;
    }
    file.flush().await?;

    Ok(total)
}

/// Guess a file extension from a URL (for naming the temp file).
pub fn url_extension(url: &str) -> &str {
    let path = url.split('?').next().unwrap_or(url);
    let filename = path.rsplit('/').next().unwrap_or("");
    if filename.ends_with(".gpkg") { return "gpkg"; }
    if filename.ends_with(".geojson") || filename.ends_with(".json") { return "geojson"; }
    if filename.ends_with(".zip") { return "zip"; }
    if filename.ends_with(".kml") { return "kml"; }
    if filename.ends_with(".fgb") { return "fgb"; }
    if filename.ends_with(".csv") { return "csv"; }
    "bin"
}
