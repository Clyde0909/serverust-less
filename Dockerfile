# ── Stage 1: Build ────────────────────────────────────────────────────────────
FROM rust:1.75-bookworm AS builder

WORKDIR /app

# Copy manifests first for dependency caching
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release 2>/dev/null || true

# Copy actual source
COPY src/ src/
COPY migrations/ migrations/
RUN cargo build --release

# ── Stage 2: Runtime ─────────────────────────────────────────────────────────
FROM debian:bookworm-slim

# Install Python for venv support
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        python3 \
        python3-venv \
        python3-pip \
        ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Create app user
RUN useradd --create-home --shell /bin/bash serverust

WORKDIR /app

# Copy binary
COPY --from=builder /app/target/release/serverust-less /usr/local/bin/serverust-less

# Copy static assets
COPY web/ web/
COPY config/ config/
COPY migrations/ migrations/

# Create data directories
RUN mkdir -p /app/data /app/venvs /app/cache \
    && chown -R serverust:serverust /app

USER serverust

# Expose default port
EXPOSE 8080

# Health check
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -f http://localhost:8080/api/v1/health || exit 1

CMD ["serverust-less"]
