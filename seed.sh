#!/usr/bin/env bash
# ============================================================
# seed.sh — Load 17 Natural Earth layers into PostGIS
#
# Uses GDAL inside the gis_ingest container (no host GDAL needed).
# Layers land in the 'gis' schema and are immediately available
# as Martin tile sources at /tiles/gis.<table>/{z}/{x}/{y}
# ============================================================
set -euo pipefail

# Load .env (needed for POSTGRES_PASSWORD, POSTGRES_USER, POSTGRES_DB)
if [ -f "$(dirname "$0")/.env" ]; then
  set -a
  # shellcheck disable=SC1091
  source "$(dirname "$0")/.env"
  set +a
else
  echo "ERROR: .env file not found. Run: cp .env.example .env && nano .env" >&2
  exit 1
fi

CONTAINER="gis_ingest"
POSTGRES_DB="${POSTGRES_DB:-gisdb}"
POSTGRES_USER="${POSTGRES_USER:-gisuser}"

# Verify the container is running
if ! docker inspect --format '{{.State.Status}}' "$CONTAINER" 2>/dev/null | grep -q "running"; then
  echo "ERROR: Container '$CONTAINER' is not running. Run 'make up' first." >&2
  exit 1
fi

# Build the PG DSN (password may contain special characters — export it separately)
export PGPASSWORD="$POSTGRES_PASSWORD"
PG_DSN="PG:host=postgis port=5432 dbname=${POSTGRES_DB} user=${POSTGRES_USER} password=${POSTGRES_PASSWORD}"

load_layer() {
  local url="$1"
  local table="$2"
  local filename
  filename="$(basename "$url")"

  printf "  %-42s " "$table"

  # Download
  docker exec "$CONTAINER" \
    wget -q --show-progress -O "/tmp/${filename}" "$url" 2>/dev/null || {
    docker exec "$CONTAINER" wget -q -O "/tmp/${filename}" "$url"
  }

  # Load via ogr2ogr (inside container — uses GDAL 3.9)
  docker exec -e PGPASSWORD="$POSTGRES_PASSWORD" "$CONTAINER" \
    ogr2ogr \
      -f "PostgreSQL" \
      "$PG_DSN" \
      "/vsizip//tmp/${filename}" \
      -nln "$table" \
      -nlt PROMOTE_TO_MULTI \
      -lco SCHEMA=gis \
      -lco GEOMETRY_NAME=geom \
      -lco FID=id \
      -lco SPATIAL_INDEX=GIST \
      -t_srs EPSG:4326 \
      --config PG_USE_COPY YES \
      -overwrite \
      -progress 2>&1 | grep -E "^[0-9]" | tail -1 || true

  # Cleanup temp file
  docker exec "$CONTAINER" rm -f "/tmp/${filename}"
  echo "✓"
}

echo ""
echo "╔══════════════════════════════════════════════════════════╗"
echo "║  GIS Stack — Natural Earth Data Seed                    ║"
echo "╚══════════════════════════════════════════════════════════╝"
echo ""
echo "Loading 17 layers into PostGIS schema 'gis'..."
echo "Download size: ~60 MB total. This may take a few minutes."
echo ""

# ── 110m (lightweight, good for low-zoom overviews) ──────────
echo "── 110m scale ──────────────────────────────────────────────"
load_layer "https://naturalearth.s3.amazonaws.com/110m_cultural/ne_110m_admin_0_countries.zip"        "ne_countries_110m"
load_layer "https://naturalearth.s3.amazonaws.com/110m_cultural/ne_110m_admin_1_states_provinces.zip" "ne_states_110m"
load_layer "https://naturalearth.s3.amazonaws.com/110m_cultural/ne_110m_populated_places.zip"          "ne_cities_110m"
load_layer "https://naturalearth.s3.amazonaws.com/110m_physical/ne_110m_rivers_lake_centerlines.zip"   "ne_rivers_110m"
load_layer "https://naturalearth.s3.amazonaws.com/110m_physical/ne_110m_lakes.zip"                     "ne_lakes_110m"
load_layer "https://naturalearth.s3.amazonaws.com/110m_physical/ne_110m_land.zip"                      "ne_land_110m"
load_layer "https://naturalearth.s3.amazonaws.com/110m_physical/ne_110m_ocean.zip"                     "ne_ocean_110m"
load_layer "https://naturalearth.s3.amazonaws.com/110m_physical/ne_110m_coastline.zip"                 "ne_coastline_110m"

echo ""
echo "── 10m scale (more detail, larger files) ───────────────────"
load_layer "https://naturalearth.s3.amazonaws.com/10m_cultural/ne_10m_admin_0_countries.zip"           "ne_countries_10m"
load_layer "https://naturalearth.s3.amazonaws.com/10m_cultural/ne_10m_airports.zip"                    "ne_airports_10m"
load_layer "https://naturalearth.s3.amazonaws.com/10m_cultural/ne_10m_ports.zip"                       "ne_ports_10m"
load_layer "https://naturalearth.s3.amazonaws.com/10m_cultural/ne_10m_roads.zip"                       "ne_roads_10m"
load_layer "https://naturalearth.s3.amazonaws.com/10m_cultural/ne_10m_urban_areas.zip"                 "ne_urban_areas_10m"
load_layer "https://naturalearth.s3.amazonaws.com/10m_cultural/ne_10m_populated_places.zip"            "ne_cities_10m"
load_layer "https://naturalearth.s3.amazonaws.com/10m_physical/ne_10m_rivers_lake_centerlines.zip"     "ne_rivers_10m"
load_layer "https://naturalearth.s3.amazonaws.com/10m_physical/ne_10m_lakes.zip"                       "ne_lakes_10m"
load_layer "https://naturalearth.s3.amazonaws.com/10m_physical/ne_10m_land.zip"                        "ne_land_10m"

echo ""
echo "╔══════════════════════════════════════════════════════════╗"
echo "║  ✅ All 17 layers loaded into gis schema                 ║"
echo "╚══════════════════════════════════════════════════════════╝"
echo ""
echo "Martin tile URLs (no restart needed):"
echo "  http://localhost/tiles/gis.ne_countries_110m/{z}/{x}/{y}"
echo "  http://localhost/tiles/gis.ne_cities_10m/{z}/{x}/{y}"
echo "  http://localhost/tiles/gis.ne_roads_10m/{z}/{x}/{y}"
echo "  ... (see all: make martin-catalog)"
echo ""
