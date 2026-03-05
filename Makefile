# ============================================================
# GIS Stack — Makefile
# ============================================================

.PHONY: up down restart logs ps health shell-db shell-gs import-vector import-raster backup restore

COMPOSE = docker compose
DB_CONTAINER = gis_postgis
GS_CONTAINER = gis_geoserver
MARTIN_CONTAINER = gis_martin

# ── Lifecycle ────────────────────────────────────────────────

up:
	@echo "▶ Starting GIS stack..."
	cp -n .env.example .env 2>/dev/null || true
	$(COMPOSE) up -d --build
	@echo "✅ Stack started. Run 'make health' to verify."

down:
	$(COMPOSE) down

restart:
	$(COMPOSE) restart

rebuild:
	$(COMPOSE) down
	$(COMPOSE) build --no-cache
	$(COMPOSE) up -d

# ── Observability ─────────────────────────────────────────────

logs:
	$(COMPOSE) logs -f

logs-db:
	$(COMPOSE) logs -f postgis

logs-gs:
	$(COMPOSE) logs -f geoserver

logs-martin:
	$(COMPOSE) logs -f martin

ps:
	$(COMPOSE) ps

health:
	@echo "\n=== Container Health ==="
	@$(COMPOSE) ps --format "table {{.Name}}\t{{.Status}}\t{{.Ports}}"
	@echo "\n=== Service Endpoints ==="
	@echo "  GeoServer:  http://localhost:$${GEOSERVER_PORT:-8080}/geoserver/web/"
	@echo "  Martin:     http://localhost:$${MARTIN_PORT:-3000}/catalog"
	@echo "  pgAdmin:    http://localhost:$${PGADMIN_PORT:-5050}"
	@echo "  Proxy:      http://localhost:$${NGINX_PORT:-80}"
	@echo "\n=== PostGIS Connection ==="
	@docker exec $(DB_CONTAINER) psql -U $${POSTGRES_USER:-gisuser} -d $${POSTGRES_DB:-gisdb} \
		-c "SELECT PostGIS_Full_Version();" 2>/dev/null || echo "  PostGIS not ready yet"

# ── Database shells ───────────────────────────────────────────

shell-db:
	docker exec -it $(DB_CONTAINER) psql -U $${POSTGRES_USER:-gisuser} -d $${POSTGRES_DB:-gisdb}

shell-gs:
	docker exec -it $(GS_CONTAINER) bash

# ── Data Import ───────────────────────────────────────────────
# Usage: make import-vector FILE=path/to/data.gpkg LAYER=my_layer
import-vector:
	@[ -n "$(FILE)" ] || (echo "Usage: make import-vector FILE=data.gpkg LAYER=layer_name" && exit 1)
	ogr2ogr \
		-f "PostgreSQL" \
		"PG:host=localhost port=$${POSTGIS_PORT:-5432} dbname=$${POSTGRES_DB:-gisdb} user=$${POSTGRES_USER:-gisuser} password=$${POSTGRES_PASSWORD}" \
		"$(FILE)" \
		$(if $(LAYER),-nln $(LAYER)) \
		-nlt PROMOTE_TO_MULTI \
		-lco SCHEMA=staging \
		-lco GEOMETRY_NAME=geom \
		-lco FID=id \
		-t_srs EPSG:4326 \
		--config PG_USE_COPY YES \
		-overwrite \
		-progress
	@echo "✅ Imported to staging schema. Move to gis schema when ready."

# Usage: make import-raster FILE=path/to/dem.tif TABLE=my_dem
import-raster:
	@[ -n "$(FILE)" ] || (echo "Usage: make import-raster FILE=dem.tif TABLE=table_name" && exit 1)
	raster2pgsql -s 4326 -I -C -M -F -t 256x256 \
		"$(FILE)" \
		raster.$(TABLE) \
		| psql -h localhost -p $${POSTGIS_PORT:-5432} -U $${POSTGRES_USER:-gisuser} -d $${POSTGRES_DB:-gisdb}
	@echo "✅ Raster imported to raster.$(TABLE)"

# ── Backup / Restore ──────────────────────────────────────────

backup:
	@mkdir -p ./backups
	docker exec $(DB_CONTAINER) pg_dump \
		-U $${POSTGRES_USER:-gisuser} \
		-d $${POSTGRES_DB:-gisdb} \
		-Fc \
		> ./backups/gisdb_$$(date +%Y%m%d_%H%M%S).dump
	@echo "✅ Backup saved to ./backups/"

restore:
	@[ -n "$(FILE)" ] || (echo "Usage: make restore FILE=backups/gisdb_YYYYMMDD.dump" && exit 1)
	docker exec -i $(DB_CONTAINER) pg_restore \
		-U $${POSTGRES_USER:-gisuser} \
		-d $${POSTGRES_DB:-gisdb} \
		--clean --if-exists \
		< "$(FILE)"
	@echo "✅ Restored from $(FILE)"

# ── Maintenance ───────────────────────────────────────────────

vacuum:
	docker exec $(DB_CONTAINER) psql -U $${POSTGRES_USER:-gisuser} -d $${POSTGRES_DB:-gisdb} \
		-c "VACUUM ANALYZE;"

martin-catalog:
	@echo "Martin tile catalog:"
	@curl -s http://localhost:$${MARTIN_PORT:-3000}/catalog | python3 -m json.tool
