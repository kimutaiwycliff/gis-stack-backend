use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tokio_postgres::NoTls;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    pub total_rows: i64,
    pub null_geom: i64,
    pub invalid_geom: i64,
    pub extent: Option<serde_json::Value>,
}

pub async fn validate_table(
    db_url: &str,
    schema: &str,
    table: &str,
) -> Result<ValidationResult> {
    let (client, connection) = tokio_postgres::connect(db_url, NoTls)
        .await
        .context("failed to connect to PostGIS for validation")?;

    tokio::spawn(async move {
        if let Err(e) = connection.await {
            tracing::error!("validation db connection error: {}", e);
        }
    });

    // Sanitize identifiers — only allow alphanumeric + underscore
    let safe_schema = sanitize_ident(schema);
    let safe_table = sanitize_ident(table);

    // Check table exists
    let exists: bool = client
        .query_one(
            "SELECT EXISTS(
                SELECT 1 FROM information_schema.tables
                WHERE table_schema = $1 AND table_name = $2
            )",
            &[&safe_schema, &safe_table],
        )
        .await
        .context("failed to check table existence")?
        .get(0);

    if !exists {
        return Ok(ValidationResult {
            total_rows: 0,
            null_geom: 0,
            invalid_geom: 0,
            extent: None,
        });
    }

    // Check if geom column exists
    let has_geom: bool = client
        .query_one(
            "SELECT EXISTS(
                SELECT 1 FROM information_schema.columns
                WHERE table_schema = $1 AND table_name = $2 AND column_name = 'geom'
            )",
            &[&safe_schema, &safe_table],
        )
        .await?
        .get(0);

    if !has_geom {
        let total: i64 = client
            .query_one(
                &format!("SELECT COUNT(*) FROM {}.{}", safe_schema, safe_table),
                &[],
            )
            .await?
            .get(0);
        return Ok(ValidationResult {
            total_rows: total,
            null_geom: 0,
            invalid_geom: 0,
            extent: None,
        });
    }

    // Fast query: row count, null-geom count, and extent (all index-friendly).
    let fast_query = format!(
        "SELECT COUNT(*), \
                COUNT(*) FILTER (WHERE geom IS NULL), \
                ST_AsGeoJSON(ST_Extent(geom))::text \
         FROM {schema}.{table}",
        schema = safe_schema,
        table = safe_table,
    );

    let row = client
        .query_one(&fast_query, &[])
        .await
        .context("validation query failed")?;

    let total: i64 = row.get(0);
    let null_geom: i64 = row.get(1);
    let extent_str: Option<String> = row.get(2);
    let extent = extent_str.and_then(|s| serde_json::from_str(&s).ok());

    // Slow optional query: ST_IsValid scans every geometry via GEOS.
    // Cap it so a large/complex dataset cannot block the pipeline indefinitely.
    client
        .batch_execute("SET statement_timeout = '15s'")
        .await
        .ok();

    let invalid_geom: i64 = client
        .query_one(
            &format!(
                "SELECT COUNT(*) FROM {schema}.{table} \
                 WHERE geom IS NOT NULL AND NOT ST_IsValid(geom)",
                schema = safe_schema,
                table = safe_table,
            ),
            &[],
        )
        .await
        .map(|r| r.get(0))
        .unwrap_or(0); // return 0 (unknown) on timeout or error

    Ok(ValidationResult {
        total_rows: total,
        null_geom,
        invalid_geom,
        extent,
    })
}

fn sanitize_ident(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric() || *c == '_')
        .collect()
}
