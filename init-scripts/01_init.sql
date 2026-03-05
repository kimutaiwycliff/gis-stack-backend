-- ============================================================
-- 01_init.sql — Runs automatically on first PostGIS container start
-- Sets up extensions, schemas, roles, and spatial metadata
-- ============================================================

-- Core spatial extensions
CREATE EXTENSION IF NOT EXISTS postgis;
CREATE EXTENSION IF NOT EXISTS postgis_topology;
CREATE EXTENSION IF NOT EXISTS postgis_raster;
CREATE EXTENSION IF NOT EXISTS fuzzystrmatch;          -- Useful for geocoding
CREATE EXTENSION IF NOT EXISTS postgis_tiger_geocoder; -- Optional, remove if not needed
CREATE EXTENSION IF NOT EXISTS address_standardizer;
CREATE EXTENSION IF NOT EXISTS pg_trgm;               -- Fast text search on attributes
CREATE EXTENSION IF NOT EXISTS btree_gist;             -- Spatial + attribute compound indexes
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";            -- UUID primary keys

-- ============================================================
-- Schemas — keep things organized
-- ============================================================
CREATE SCHEMA IF NOT EXISTS gis;        -- Your main spatial data
CREATE SCHEMA IF NOT EXISTS raster;     -- Raster datasets
CREATE SCHEMA IF NOT EXISTS staging;    -- Temp import area (ogr2ogr dumps here first)
CREATE SCHEMA IF NOT EXISTS audit;      -- Change tracking

-- ============================================================
-- Application role — least-privilege for GeoServer & Martin
-- ============================================================
DO $$
BEGIN
  IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'gis_reader') THEN
    CREATE ROLE gis_reader;
  END IF;
  IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'gis_writer') THEN
    CREATE ROLE gis_writer;
  END IF;
END
$$;

-- gis_reader: SELECT only (for Martin tiles, public WMS)
GRANT CONNECT ON DATABASE gisdb TO gis_reader;
GRANT USAGE ON SCHEMA gis, public, raster TO gis_reader;
ALTER DEFAULT PRIVILEGES IN SCHEMA gis GRANT SELECT ON TABLES TO gis_reader;
ALTER DEFAULT PRIVILEGES IN SCHEMA raster GRANT SELECT ON TABLES TO gis_reader;

-- gis_writer: read + write (for GeoServer, data ingestion)
GRANT gis_reader TO gis_writer;
GRANT USAGE ON SCHEMA staging TO gis_writer;
ALTER DEFAULT PRIVILEGES IN SCHEMA gis GRANT INSERT, UPDATE, DELETE ON TABLES TO gis_writer;
ALTER DEFAULT PRIVILEGES IN SCHEMA staging GRANT ALL ON TABLES TO gis_writer;

-- Grant the main user both roles
GRANT gis_reader, gis_writer TO gisuser;

-- ============================================================
-- Spatial index helper function
-- Usage: SELECT create_spatial_index('my_table', 'geom');
-- ============================================================
CREATE OR REPLACE FUNCTION public.create_spatial_index(
    p_table  TEXT,
    p_column TEXT DEFAULT 'geom',
    p_schema TEXT DEFAULT 'gis'
)
RETURNS VOID AS $$
BEGIN
    EXECUTE format(
        'CREATE INDEX IF NOT EXISTS idx_%s_%s_gist ON %I.%I USING GIST (%I)',
        p_table, p_column, p_schema, p_table, p_column
    );
    RAISE NOTICE 'Spatial index created on %.%.%', p_schema, p_table, p_column;
END;
$$ LANGUAGE plpgsql;

-- ============================================================
-- Audit trigger function — track who changed what
-- ============================================================
CREATE TABLE IF NOT EXISTS audit.logged_actions (
    id          BIGSERIAL PRIMARY KEY,
    schema_name TEXT NOT NULL,
    table_name  TEXT NOT NULL,
    action      TEXT NOT NULL CHECK (action IN ('INSERT','UPDATE','DELETE')),
    row_data    JSONB,
    changed_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    changed_by  TEXT NOT NULL DEFAULT current_user
);

CREATE OR REPLACE FUNCTION audit.log_change()
RETURNS TRIGGER AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        INSERT INTO audit.logged_actions(schema_name, table_name, action, row_data)
        VALUES (TG_TABLE_SCHEMA, TG_TABLE_NAME, TG_OP, to_jsonb(OLD));
    ELSE
        INSERT INTO audit.logged_actions(schema_name, table_name, action, row_data)
        VALUES (TG_TABLE_SCHEMA, TG_TABLE_NAME, TG_OP, to_jsonb(NEW));
    END IF;
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

-- ============================================================
-- PostGIS tuning for spatial queries
-- (supplements postgresql.conf — applied at session level here)
-- ============================================================
ALTER DATABASE gisdb SET work_mem = '64MB';
ALTER DATABASE gisdb SET maintenance_work_mem = '256MB';
ALTER DATABASE gisdb SET max_parallel_workers_per_gather = 4;

-- ============================================================
-- Verify setup
-- ============================================================
SELECT PostGIS_Full_Version();
