# GIS Backend Stack

> **👉 Open `GUIDE.html` in your browser for the full interactive operations playbook.**
> It covers every workflow: PostGIS imports, spatial queries, GeoServer WMS/WFS/WMTS, Martin vector tiles, MapLibre integration, ogr2ogr cheatsheet, backups, and debugging.

---

## Stack

| Service | Image | Purpose | Port |
|---|---|---|---|
| **PostGIS** | `postgis/postgis:16-3.4-alpine` | Spatial database | `5433` (host) |
| **GeoServer** | `kartoza/geoserver:2.24.2` | OGC WMS/WFS/WCS/WMTS | `8080` |
| **Martin** | `ghcr.io/maplibre/martin:v0.13.0` | MVT vector tiles | `3000` |
| **GIS Ingest** | (built locally, Rust + GDAL) | Web UI for data upload & URL fetch | `8000` |
| **pgAdmin** | `dpage/pgadmin4` | DB management UI | `5050` |
| **Nginx** | `nginx:1.25-alpine` | Reverse proxy (port 80) | `80` |

---

## Quick Start

```bash
# 1. Set your passwords
cp .env.example .env
nano .env          # fill in all CHANGE_ME values

# 2. Start (first run builds the Rust ingest image — takes ~10 min)
make up

# 3. Verify (~2 min for GeoServer to fully boot)
make health

# 4. (Optional) Load sample Natural Earth data
make seed
```

### Service URLs after startup

| Service | URL |
|---|---|
| GeoServer UI | http://localhost/geoserver/web/ |
| Martin tile catalog | http://localhost/tiles/catalog |
| **GIS Ingest UI** | **http://localhost/ingest** |
| pgAdmin | http://localhost/pgadmin/ |
| PostGIS (direct) | `localhost:5433` |

---

## GIS Ingest — Browser-Based Data Loading

The stack includes a Rust-powered ingest service with a web UI for loading spatial data
into PostGIS without touching the command line.

Open **http://localhost/ingest** in your browser.

### What it supports

| Format | Notes |
|---|---|
| GeoJSON (`.geojson`, `.json`) | Single or FeatureCollection |
| Shapefile (`.zip` containing `.shp`) | Must be zipped |
| GeoPackage (`.gpkg`) | Multi-layer supported |
| KML / KMZ | Google Earth files |
| CSV with lat/lon columns | Detected automatically |
| Any GDAL-readable format | Uses GDAL 3.9 drivers |

### Two ways to load data

**Upload a file**
1. Go to the **Upload File** tab
2. Drag and drop a file (or click to browse) — up to 500 MB
3. Click **Inspect** — the service runs `ogrinfo` and shows format, CRS, feature count, extent, and field list
4. Choose a target schema (`gis`, `staging`, `raster`, or `public`), table name, and load mode (`overwrite` or `append`)
5. Click **Load** — a live progress log streams via SSE while `ogr2ogr` runs
6. When complete, PostGIS validation stats appear: total rows, null geometries, invalid geometries, and extent

**Fetch from URL**
1. Go to the **Fetch from URL** tab
2. Paste a direct download link (GeoJSON, zip, GPKG, etc.)
   - Quick-fill buttons for Natural Earth 110m countries and Geofabrik extracts are provided
3. Same Inspect → Load flow as above

### What happens under the hood

```
Browser
  │  POST /ingest/api/inspect  (multipart file or JSON url)
  │  POST /ingest/api/load     (schema / table / mode selection)
  │  GET  /ingest/api/jobs/:id/events  (SSE progress stream)
  ▼
gis-ingest (Rust / Axum)
  ├─ ogrinfo subprocess  → format, CRS, feature count, columns
  ├─ ogr2ogr subprocess  → loads to PostGIS, streams stdout/stderr as SSE
  └─ tokio-postgres      → ST_IsValid, ST_Extent, NULL geom counts
  ▼
PostGIS (gis schema by default)
```

Data is always:
- Re-projected to **EPSG:4326** (`-t_srs EPSG:4326`)
- Geometry promoted to Multi (`-nlt PROMOTE_TO_MULTI`)
- Indexed with a **GIST spatial index**
- Loaded via `PG_USE_COPY YES` for performance

### After loading

The loaded table is immediately available as:
- A **Martin tile source** at `http://localhost/tiles/<schema>.<table>/{z}/{x}/{y}`
- A **GeoServer layer** (add it manually in the GeoServer UI)
- Queryable from **pgAdmin** or `make shell-db`

```sql
-- Verify in psql
SELECT COUNT(*), ST_Extent(geom) FROM gis.my_table;
```

---

## File Structure

```
gis-stack/
├── GUIDE.html               ← Full interactive operations guide (open in browser)
├── docker-compose.yml       ← Main stack definition
├── .env.example             ← Copy to .env and fill passwords
├── Makefile                 ← All common commands (make up, make health, etc.)
├── seed.sh                  ← Downloads & loads 17 Natural Earth layers into PostGIS
├── gis-ingest/              ← Rust data ingestion microservice
│   ├── Cargo.toml
│   ├── Dockerfile           ← Multi-stage: rust:1.85-bookworm → gdal:ubuntu-small-3.9.0
│   ├── src/
│   │   ├── main.rs          ← Axum router, SSE job streaming
│   │   ├── inspect.rs       ← ogrinfo subprocess + URL download
│   │   ├── load.rs          ← ogr2ogr subprocess + SSE progress
│   │   ├── validate.rs      ← PostGIS ST_IsValid / ST_Extent checks
│   │   ├── jobs.rs          ← In-memory job store (DashMap + broadcast channels)
│   │   └── error.rs         ← AppError → JSON HTTP responses
│   └── static/
│       └── index.html       ← Single-file UI (Tailwind CDN, no build step)
├── init-scripts/
│   └── 01_init.sql          ← Auto-runs on first PostGIS start (extensions, schemas, roles)
├── martin-config/
│   └── martin.yaml          ← Martin tile server config (auto-discover, CORS, pool)
├── nginx/
│   └── nginx.conf           ← Reverse proxy routing
├── pgadmin-config/
│   └── servers.json         ← Auto-registers PostGIS in pgAdmin
└── tiles/                   ← Drop .mbtiles / .pmtiles files here for Martin to serve
```

---

## Common Commands

```bash
make up              # Start stack (builds gis-ingest on first run)
make down            # Stop stack (data persists)
make health          # Status + all service URLs
make logs            # Tail all logs
make shell-db        # psql into PostGIS
make backup          # Dump DB to ./backups/
make vacuum          # VACUUM ANALYZE database
make martin-catalog  # List all Martin tile sources

# Load sample Natural Earth data (17 layers, ~110m and 10m scale)
make seed

# gis-ingest specific
make ingest-logs     # Tail gis-ingest container logs
make ingest-build    # Force rebuild the gis-ingest Docker image

# Import vector data via CLI (requires ogr2ogr on host)
make import-vector FILE=data.gpkg LAYER=my_layer

# Import raster data (requires raster2pgsql on host)
make import-raster FILE=dem.tif TABLE=elevation_model

# Restore from backup
make restore FILE=backups/gisdb_20241201.dump
```

---

## Architecture

```
Browser / QGIS / ArcGIS
        │
        ▼
    Nginx :80
   ┌────────────────────────────────────┐
   │  /geoserver/*  → geoserver:8080    │  OGC WMS, WFS, WCS, WMTS
   │  /tiles/*      → martin:3000       │  MVT vector tiles
   │  /ingest       → gis-ingest:8000   │  Upload/fetch UI + REST API
   │  /pgadmin/*    → pgadmin:80        │  DB management
   └────────────────────────────────────┘
        │                │           │
   GeoServer           Martin    gis-ingest
        │                │           │
        └────────┬────────┘           │
                 ▼                    │
             PostGIS :5432 ←──────────┘
         (gis + raster schemas)
```

**GeoServer** — OGC standards, raster serving, SLD cartography, QGIS/ArcGIS clients
**Martin** — High-speed MVT tiles for MapLibre GL / Mapbox web maps, auto-discovers PostGIS tables
**GIS Ingest** — Rust/Axum service; upload or fetch any GDAL-readable format, inspect metadata, load to PostGIS with live SSE progress, and run automatic PostGIS validation

---

## Schemas (pre-configured in PostGIS)

| Schema | Purpose |
|---|---|
| `public` | PostGIS extensions and metadata |
| `gis` | Your primary spatial data (default ingest target) |
| `raster` | Raster datasets (raster2pgsql imports) |
| `staging` | Landing zone — inspect before promoting to `gis` |
| `audit` | Change log for all edits |

---

## Production Hardening Checklist

- [ ] Change ALL passwords in `.env`
- [ ] Add SSL to Nginx (Let's Encrypt)
- [ ] Restrict `CORS_ALLOWED_ORIGINS` to your domains
- [ ] Disable or VPN-gate pgAdmin
- [ ] Schedule daily backups: `0 2 * * * cd /path/to/gis-stack && make backup`
- [ ] Seed GeoWebCache for high-traffic WMS layers
- [ ] Add PgBouncer for high-concurrency PostGIS connections
- [ ] Restrict `/ingest` behind auth or VPN (no auth by default)

---

## Requirements

- Docker Desktop (Mac M1/M2) or Docker Engine (Linux)
- Docker Compose v2+
- 4 GB+ RAM recommended (GeoServer needs ~1.5 GB; Rust build needs ~2 GB)
- ogr2ogr / GDAL on host for `make import-vector` (optional — not needed for the web UI)
