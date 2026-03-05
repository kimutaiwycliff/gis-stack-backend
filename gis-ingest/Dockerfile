# ============================================================
# Stage 1: Build the Rust binary
# ============================================================
FROM rust:1.85-bookworm AS builder

WORKDIR /app

# Cache dependency compilation layer separately
COPY Cargo.toml ./
RUN mkdir src && echo 'fn main(){}' > src/main.rs && \
    cargo build --release && \
    rm -rf src

# Build the real source
COPY src ./src
COPY static ./static
RUN touch src/main.rs && cargo build --release

# ============================================================
# Stage 2: Runtime — GDAL image (has ogrinfo + ogr2ogr)
# ============================================================
FROM ghcr.io/osgeo/gdal:ubuntu-small-3.9.0

RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates wget && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/gis-ingest /app/gis-ingest
COPY --from=builder /app/static /app/static

ENV RUST_LOG=info
ENV INGEST_PORT=8000

EXPOSE 8000

HEALTHCHECK --interval=15s --timeout=5s --retries=5 --start-period=30s \
    CMD wget -qO- http://127.0.0.1:8000/ingest/health || exit 1

CMD ["/app/gis-ingest"]
