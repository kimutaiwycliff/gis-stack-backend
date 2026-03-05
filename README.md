# GIS Backend Stack

> **👉 Open `GUIDE.html` in your browser for the full interactive operations playbook.**  
> It covers every workflow: PostGIS imports, spatial queries, GeoServer WMS/WFS/WMTS, Martin vector tiles, MapLibre integration, ogr2ogr cheatsheet, backups, and debugging.

---

## Stack

| Service | Image | Purpose | Port |
|---|---|---|---|
| **PostGIS** | `postgis/postgis:16-3.4-alpine` | Spatial database | `5432` |
| **GeoServer** | `kartoza/geoserver:2.24.2` | OGC WMS/WFS/WCS/WMTS | `8080` |
| **Martin** | `ghcr.io/maplibre/martin:v0.13.0` | MVT vector tiles | `3000` |
| **pgAdmin** | `dpage/pgadmin4` | DB management UI | `5050` |
| **Nginx** | `nginx:1.25-alpine` | Reverse proxy (port 80) | `80` |

---

## Quick Start

```bash
# 1. Set your passwords
cp .env.example .env
nano .env          # fill in all CHANGE_ME values

# 2. Start
make up

# 3. Verify (~2 min for GeoServer to fully boot)
make health
```

### Service URLs after startup

| Service | URL |
|---|---|
| GeoServer UI | http://localhost/geoserver/web/ |
| Martin tile catalog | http://localhost/tiles/catalog |
| pgAdmin | http://localhost/pgadmin/ |
| PostGIS (direct) | `localhost:5432` |

---

## File Structure

```
gis-stack/
├── GUIDE.html               ← Full interactive operations guide (open in browser)
├── docker-compose.yml       ← Main stack definition
├── .env.example             ← Copy to .env and fill passwords
├── Makefile                 ← All common commands (make up, make health, etc.)
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
make up              # Start stack
make down            # Stop stack (data persists)
make health          # Status + all service URLs
make logs            # Tail all logs
make shell-db        # psql into PostGIS
make backup          # Dump DB to ./backups/
make vacuum          # VACUUM ANALYZE database
make martin-catalog  # List all Martin tile sources

# Import vector data (requires ogr2ogr on host)
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
   ┌──────────────────────────────┐
   │  /geoserver/*  → :8080       │  OGC WMS, WFS, WCS, WMTS
   │  /tiles/*      → :3000       │  MVT vector tiles
   │  /pgadmin/*    → :5050       │  DB management
   └──────────────────────────────┘
        │                │
   GeoServer           Martin
        │                │
        └────────┬───────┘
                 ▼
             PostGIS :5432
         (gis + raster schemas)
```

**GeoServer** — OGC standards, raster serving, SLD cartography, QGIS/ArcGIS clients  
**Martin** — High-speed MVT tiles for MapLibre GL / Mapbox web maps, auto-discovers PostGIS tables  

---

## Schemas (pre-configured in PostGIS)

| Schema | Purpose |
|---|---|
| `public` | PostGIS extensions and metadata |
| `gis` | Your primary spatial data |
| `raster` | Raster datasets (raster2pgsql imports) |
| `staging` | ogr2ogr landing zone — inspect before promoting |
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

---

## Requirements

- Docker Desktop (Mac M1/M2) or Docker Engine (Linux)
- Docker Compose v2+
- 4GB+ RAM recommended (GeoServer needs ~1.5GB)
- ogr2ogr / GDAL on host for `make import-vector` (optional but recommended)
