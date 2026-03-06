# GIS Backend Stack

A self-contained, Docker-based spatial data platform providing PostGIS, GeoServer (OGC WMS/WFS/WMTS), Martin (MVT vector tiles), a browser-based data ingest tool, and pgAdmin — all unified behind an Nginx reverse proxy on port 80.

> **For the full interactive operations playbook** (spatial queries, WMS/WFS cheatsheets, MapLibre integration, backups, ogr2ogr reference), open `GUIDE.html` in your browser.

---

## Stack overview

| Service | Image | Purpose | Internal port |
|---|---|---|---|
| **PostGIS** | `postgis/postgis:16-3.4-alpine` | Spatial database | 5432 |
| **GeoServer** | `kartoza/geoserver:2.24.2` | OGC WMS / WFS / WCS / WMTS | 8080 |
| **Martin** | `ghcr.io/maplibre/martin:v0.13.0` | MVT vector tiles | 3000 |
| **GIS Ingest** | Built locally (Rust + GDAL 3.9) | Web UI for upload & URL fetch | 8000 |
| **pgAdmin** | `dpage/pgadmin4:8.5` | DB management UI | 80 (internal) |
| **Nginx** | `nginx:1.25-alpine` | Reverse proxy (single entry point) | **80 (host)** |

Everything is accessed through Nginx on **port 80** (or 443 with SSL).

---

## Quick start (local machine)

```bash
# 1. Copy and fill in secrets
cp .env.example .env
nano .env            # change all CHANGE_ME values

# 2. Start (first run builds the Rust ingest image — ~5-10 min)
make up

# 3. Wait ~2 min for GeoServer to boot, then verify
make health

# 4. (Optional) Load 17 Natural Earth sample layers
make seed
```

### URLs after startup

| Service | URL |
|---|---|
| GeoServer UI | http://localhost/geoserver/web/ |
| Martin tile catalog | http://localhost/tiles/catalog |
| GIS Ingest UI | http://localhost/ingest |
| pgAdmin | http://localhost/pgadmin/ |
| PostGIS (direct) | `localhost:5432` (host mapped) |

---

## Deploying on a remote server

### 1. Server requirements

| Requirement | Minimum | Recommended |
|---|---|---|
| OS | Ubuntu 22.04 LTS | Ubuntu 24.04 LTS |
| CPU | 2 cores | 4+ cores |
| RAM | 4 GB | 8 GB (GeoServer needs ~1.5 GB alone) |
| Disk | 20 GB | 50 GB+ (depends on data volume) |
| Docker Engine | v24+ | latest |
| Docker Compose | v2+ (included in Docker Engine) | latest |
| Architecture | **x86_64 / AMD64** (DigitalOcean, Linode, Hetzner, AWS EC2 Intel/AMD) | same |

> **ARM servers (AWS Graviton, Ampere):** All services in `docker-compose.yml` have `platform: linux/amd64`. On an ARM host, remove or comment out all `platform:` lines to use native ARM images instead of emulation.

### 2. Install Docker on the server

```bash
# SSH into your server
ssh user@YOUR_SERVER_IP

# Install Docker (Ubuntu)
curl -fsSL https://get.docker.com | sh
sudo usermod -aG docker $USER
newgrp docker          # activate group membership without re-login

# Verify
docker --version
docker compose version
```

### 3. Clone and configure

```bash
# Clone the repo
git clone https://github.com/YOUR_USERNAME/gis-stack.git
cd gis-stack

# Copy env template and fill in all secrets
cp .env.example .env
nano .env
```

**Required `.env` values** — change every `CHANGE_ME`:

```dotenv
POSTGRES_PASSWORD=<strong-random-password>
GEOSERVER_ADMIN_PASSWORD=<strong-random-password>
PGADMIN_PASSWORD=<strong-random-password>

# Optional: your email for pgAdmin login
PGADMIN_EMAIL=you@yourdomain.com
```

Generate strong passwords:
```bash
openssl rand -base64 24
```

### 4. Start the stack

```bash
# First run: builds the Rust ingest image (~5-10 min), pulls all other images
docker compose up -d

# Monitor startup (GeoServer takes ~90 s to fully boot)
docker compose logs -f nginx geoserver

# Check all services are healthy
docker compose ps
```

Once you see all containers in a `healthy` state the stack is ready.

### 5. Access from your browser

Replace `YOUR_SERVER_IP` with your server's public IP (or domain):

| Service | URL |
|---|---|
| GeoServer UI | `http://YOUR_SERVER_IP/geoserver/web/` |
| Martin tile catalog | `http://YOUR_SERVER_IP/tiles/catalog` |
| GIS Ingest UI | `http://YOUR_SERVER_IP/ingest` |
| pgAdmin | `http://YOUR_SERVER_IP/pgadmin/` |

---

## SSL / HTTPS with a domain name

This is strongly recommended for production. The approach below uses Certbot (Let's Encrypt) to get a free certificate and add it to the Nginx container.

### Option A — Certbot on the host (simplest)

```bash
# Install Certbot on the host
sudo apt install -y certbot

# Stop Nginx temporarily to free port 80 for the challenge
docker compose stop nginx

# Get the certificate (replace with your domain)
sudo certbot certonly --standalone -d gis.yourdomain.com

# Restart Nginx
docker compose start nginx
```

Certificates are saved to `/etc/letsencrypt/live/gis.yourdomain.com/`.

### Option B — Mount certs into the Nginx container

1. Add a volume to the `nginx` service in `docker-compose.yml`:

```yaml
nginx:
  volumes:
    - ./nginx/nginx.conf:/etc/nginx/nginx.conf:ro
    - ./nginx/conf.d:/etc/nginx/conf.d:ro
    - /etc/letsencrypt:/etc/letsencrypt:ro   # ← add this
```

2. Create `nginx/conf.d/ssl.conf`:

```nginx
server {
    listen 443 ssl;
    server_name gis.yourdomain.com;

    ssl_certificate     /etc/letsencrypt/live/gis.yourdomain.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/gis.yourdomain.com/privkey.pem;
    ssl_protocols       TLSv1.2 TLSv1.3;
    ssl_ciphers         HIGH:!aNULL:!MD5;

    # Include the same proxy rules as the HTTP block
    # (copy location blocks from nginx.conf, or include a shared file)
    include /etc/nginx/conf.d/locations.conf;
}

# Redirect HTTP → HTTPS
server {
    listen 80;
    server_name gis.yourdomain.com;
    return 301 https://$host$request_uri;
}
```

3. Rebuild Nginx: `docker compose restart nginx`

After this, all services are available on `https://gis.yourdomain.com/...`

### Auto-renew certificates

```bash
# Add to crontab (runs twice daily, standard Let's Encrypt recommendation)
echo "0 */12 * * * root certbot renew --quiet && docker compose -f /path/to/gis-stack/docker-compose.yml restart nginx" | sudo tee -a /etc/crontab
```

---

## URL reference (for frontend developers)

All URLs below use `http://YOUR_HOST` — swap for `https://your.domain` in production.

### Martin — Vector tiles (MVT)

Martin auto-discovers every table in the `gis` and `public` schemas that has a geometry column.

| What | URL pattern |
|---|---|
| Tile catalog (all sources) | `http://YOUR_HOST/tiles/catalog` |
| TileJSON for a layer | `http://YOUR_HOST/tiles/{schema}.{table}` |
| Actual tile | `http://YOUR_HOST/tiles/{schema}.{table}/{z}/{x}/{y}` |

**Examples:**

```
# TileJSON (metadata, bounds, min/maxzoom)
http://YOUR_HOST/tiles/gis.countries

# Tiles
http://YOUR_HOST/tiles/gis.countries/{z}/{x}/{y}
http://YOUR_HOST/tiles/gis.airports/{z}/{x}/{y}
http://YOUR_HOST/tiles/gis.roads/{z}/{x}/{y}
```

**MapLibre GL JS integration:**

```javascript
map.addSource('countries', {
  type: 'vector',
  tiles: ['http://YOUR_HOST/tiles/gis.countries/{z}/{x}/{y}'],
  minzoom: 0,
  maxzoom: 14,
});

map.addLayer({
  id: 'countries-fill',
  type: 'fill',
  source: 'countries',
  'source-layer': 'gis.countries',   // source-layer = schema.table
  paint: { 'fill-color': '#3a86ff', 'fill-opacity': 0.4 },
});
```

**CORS:** Martin tiles have `Access-Control-Allow-Origin: *` — no proxy changes needed for any origin.

**Tip:** After loading a layer via the GIS Ingest UI, its Martin tile URL is immediately live at `http://YOUR_HOST/tiles/{schema}.{table}/{z}/{x}/{y}`. No restart required.

---

### GeoServer — OGC services (WMS / WFS / WMTS / WCS)

GeoServer requires you to configure a workspace and layer first (via the UI at `/geoserver/web/` or the REST API). Once a layer is published:

#### WMS — raster map images (for display)

```
# GetMap (get a PNG map image)
http://YOUR_HOST/geoserver/{workspace}/wms?
  service=WMS&
  version=1.1.1&
  request=GetMap&
  layers={workspace}:{layer}&
  styles=&
  bbox={minx},{miny},{maxx},{maxy}&
  width=800&
  height=600&
  srs=EPSG:4326&
  format=image/png

# GetCapabilities (discover all layers)
http://YOUR_HOST/geoserver/{workspace}/wms?service=WMS&request=GetCapabilities

# GetFeatureInfo (click-to-identify)
http://YOUR_HOST/geoserver/{workspace}/wms?
  service=WMS&version=1.1.1&
  request=GetFeatureInfo&
  layers={workspace}:{layer}&
  query_layers={workspace}:{layer}&
  info_format=application/json&
  x=256&y=256&width=512&height=512&
  bbox={minx},{miny},{maxx},{maxy}&
  srs=EPSG:4326
```

**Leaflet.js integration:**

```javascript
const wmsLayer = L.tileLayer.wms('http://YOUR_HOST/geoserver/gis/wms', {
  layers: 'gis:countries',
  format: 'image/png',
  transparent: true,
  version: '1.1.1',
});
wmsLayer.addTo(map);
```

#### WFS — vector feature data (for analysis / download)

```
# GetFeature as GeoJSON
http://YOUR_HOST/geoserver/{workspace}/wfs?
  service=WFS&
  version=2.0.0&
  request=GetFeature&
  typeName={workspace}:{layer}&
  outputFormat=application/json&
  count=100

# Filter by attribute (CQL)
http://YOUR_HOST/geoserver/{workspace}/wfs?
  service=WFS&version=2.0.0&
  request=GetFeature&
  typeName={workspace}:{layer}&
  outputFormat=application/json&
  CQL_FILTER=population>1000000

# Spatial filter (BBOX)
http://YOUR_HOST/geoserver/{workspace}/wfs?
  service=WFS&version=2.0.0&
  request=GetFeature&
  typeName={workspace}:{layer}&
  outputFormat=application/json&
  BBOX={minx},{miny},{maxx},{maxy},EPSG:4326
```

#### WMTS — pre-rendered tile cache (fastest for basemaps)

```
# Capabilities
http://YOUR_HOST/geoserver/gwc/service/wmts?REQUEST=GetCapabilities

# Tile request
http://YOUR_HOST/geoserver/gwc/service/wmts?
  SERVICE=WMTS&REQUEST=GetTile&
  VERSION=1.0.0&
  LAYER={workspace}:{layer}&
  STYLE=&
  TILEMATRIXSET=EPSG:900913&
  TILEMATRIX=EPSG:900913:{z}&
  TILEROW={y}&TILECOL={x}&
  FORMAT=image/png

# MapLibre / OpenLayers slippy-map format
http://YOUR_HOST/geoserver/gwc/service/tms/1.0.0/{workspace}:{layer}@EPSG:900913@png/{z}/{x}/{-y}.png
```

**QGIS / ArcGIS clients:** Add connection → WMS/WFS/WMTS → use `http://YOUR_HOST/geoserver/{workspace}/wms` (or `wfs`, `wmts`).

**CORS:** GeoServer has `CORS_ENABLED=true` and `CORS_ALLOWED_ORIGINS=*` set in `docker-compose.yml` — browser requests from any origin are allowed.

---

### Setting up a GeoServer layer (quick steps)

After loading data via the GIS Ingest UI:

1. Go to `http://YOUR_HOST/geoserver/web/` and log in
2. **Stores → Add new Store → PostGIS** — connection parameters:
   - Host: `postgis`
   - Port: `5432`
   - Database: `gisdb`
   - User: `gisuser`
   - Password: (your `POSTGRES_PASSWORD`)
   - Schema: `gis`
3. **Layers → Add new layer** → select the store → publish the table
4. On the **Data** tab, click "Compute from data" for the bounding box
5. Click **Save**

The layer is now available on all OGC endpoints.

---

### GIS Ingest API (for programmatic use)

The ingest tool also exposes a REST API used by its browser UI — you can call it from scripts:

```bash
# Inspect a remote file
curl -X POST http://YOUR_HOST/ingest/api/inspect \
  -F url=https://example.com/data.geojson

# Inspect a local file
curl -X POST http://YOUR_HOST/ingest/api/inspect \
  -F file=@/path/to/local/data.gpkg

# Start a load job
curl -X POST http://YOUR_HOST/ingest/api/load \
  -H "Content-Type: application/json" \
  -d '{"source_path":"/tmp/...", "schema":"gis", "table":"my_layer", "mode":"overwrite"}'

# Stream job progress (Server-Sent Events)
curl -N http://YOUR_HOST/ingest/api/jobs/{job_id}/events
```

---

## Workflow: load data → serve tiles → display in a map

```
1. Open http://YOUR_HOST/ingest
2. Upload a .gpkg / .geojson / .zip (Shapefile) — or paste a URL
3. Click Inspect → review CRS, feature count, field list
4. Choose schema=gis, enter a table name, click Load
5. Martin auto-publishes the new table immediately:
   http://YOUR_HOST/tiles/gis.<your_table>/{z}/{x}/{y}
6. Add to MapLibre / Leaflet / OpenLayers using the URLs above
7. (Optional) Publish in GeoServer for WMS/WFS/WMTS access
```

---

## Useful make commands

```bash
make up              # Start stack
make down            # Stop stack (data volumes persist)
make health          # Status + all service URLs
make logs            # Tail all logs
make logs-gs         # Tail GeoServer logs only
make ingest-logs     # Tail GIS Ingest logs
make shell-db        # psql session into PostGIS
make backup          # Dump DB to ./backups/
make vacuum          # VACUUM ANALYZE the database
make martin-catalog  # Print all Martin tile sources as JSON
make seed            # Download and load 17 Natural Earth sample layers
make ingest-build    # Rebuild gis-ingest Docker image from scratch
```

---

## File structure

```
gis-stack/
├── docker-compose.yml       ← All 6 services, volumes, network
├── .env.example             ← Template — copy to .env and fill secrets
├── Makefile                 ← Shortcut commands
├── GUIDE.html               ← Interactive operations playbook
├── gis-ingest/
│   ├── Dockerfile           ← Multi-stage: rust:1.85-bookworm → gdal:ubuntu-small-3.9.0
│   ├── Cargo.toml
│   └── src/                 ← Axum web server, ogrinfo/ogr2ogr subprocesses, SSE jobs
│       └── ...
├── static/
│   └── index.html           ← Browser UI (Tailwind CDN, no build step)
├── init-scripts/
│   └── 01_init.sql          ← PostGIS extensions, schemas, roles (auto-runs on first start)
├── martin-config/
│   └── martin.yaml          ← Auto-discovers gis.* and public.* tables
├── nginx/
│   ├── nginx.conf           ← Reverse proxy routing
│   └── conf.d/              ← Drop extra Nginx configs here (e.g. ssl.conf)
├── pgadmin-config/
│   └── servers.json         ← Auto-registers PostGIS connection in pgAdmin
└── tiles/                   ← Drop .mbtiles / .pmtiles files here for Martin to serve
```

---

## PostGIS schemas (pre-configured)

| Schema | Purpose |
|---|---|
| `gis` | Primary spatial data — default ingest target |
| `staging` | Landing zone for imports before promoting to `gis` |
| `raster` | Raster datasets (raster2pgsql imports) |
| `public` | PostGIS extension metadata |
| `audit` | Change log for all edits |

Database roles `gis_reader` (SELECT only) and `gis_writer` are pre-created. Martin uses `gis_reader` access implicitly through `gisuser`.

---

## Production hardening checklist

- [ ] Change **all** passwords in `.env` — never leave `CHANGE_ME` values
- [ ] **Add SSL** to Nginx (see SSL section above)
- [ ] Point DNS `A` record to your server IP before running Certbot
- [ ] Change `CORS_ALLOWED_ORIGINS` from `*` to your specific frontend domain(s):
  ```yaml
  # docker-compose.yml → geoserver environment
  CORS_ALLOWED_ORIGINS: "https://yourapp.com,https://app.yourapp.com"
  ```
- [ ] **Gate pgAdmin** — either VPN it or remove it entirely in production:
  ```yaml
  # Comment out pgadmin service + its Nginx location block
  ```
- [ ] **Gate the GIS Ingest UI** — it has no authentication by default. Protect it with Nginx basic auth or restrict to your IP:
  ```nginx
  # nginx/conf.d/ingest-auth.conf
  location /ingest {
    auth_basic "Restricted";
    auth_basic_user_file /etc/nginx/.htpasswd;
    # ... existing proxy settings
  }
  ```
  Generate: `htpasswd -c nginx/.htpasswd yourusername`
- [ ] Schedule daily backups:
  ```bash
  echo "0 2 * * * cd /home/user/gis-stack && make backup" | crontab -
  ```
- [ ] Set `restart: always` (vs `unless-stopped`) if you need auto-start on reboot
- [ ] Increase GeoServer memory for production: `GEOSERVER_MAXIMUM_MEMORY=2048M`
- [ ] Seed GeoWebCache for high-traffic WMS layers (GeoServer → Tile Caching → Seed)

---

## Troubleshooting

**GeoServer is slow or won't start**
```bash
make logs-gs   # check Java OOM errors
# Increase memory in .env:
# GEOSERVER_MAXIMUM_MEMORY=2048M
```

**Martin shows no tiles / empty catalog**
```bash
docker compose exec martin wget -qO- http://localhost:3000/catalog | python3 -m json.tool
# If empty — check that the table has a geometry column named 'geom' in the gis or public schema
```

**Can't connect to PostGIS from outside the container**
```bash
# PostGIS is mapped to localhost:5432 (host) — not exposed to the internet by default
# To check from the host:
psql -h localhost -p 5432 -U gisuser -d gisdb
# To open it to external connections — add firewall rule (only do this if intentional):
# ufw allow 5432  (NOT recommended in production)
```

**Ingest job shows "Connection closed"**
```bash
make ingest-logs   # check for ogr2ogr errors, temp file issues
# The temp file is stored in /tmp — make sure the container has enough disk space
docker system df
```

**SSL certificate not renewing**
```bash
sudo certbot renew --dry-run   # test renewal without applying
```

**Full reset (WARNING: destroys all data)**
```bash
docker compose down -v   # -v removes all volumes including PostGIS data
docker compose up -d
```

---

## Requirements summary

- Docker Engine v24+ with Docker Compose v2+
- 4 GB RAM minimum (8 GB recommended)
- No host-side GDAL required — the ingest UI handles everything inside the container
- Optional: `ogr2ogr` on the host for `make import-vector`
