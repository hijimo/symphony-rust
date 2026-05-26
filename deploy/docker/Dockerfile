# syntax=docker/dockerfile:1.7

FROM rust:1.82-bookworm AS rust-builder
WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends build-essential libssl-dev pkg-config \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY rust-platform/ rust-platform/
COPY web-platform/ web-platform/

RUN cargo build --release -p web-platform -p symphony-platform

FROM node:20-alpine AS frontend-builder
WORKDIR /app

COPY web-frontend/package.json web-frontend/package-lock.json ./
RUN npm ci

COPY web-frontend/ ./
RUN npm run build

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl git libssl3 \
    && rm -rf /var/lib/apt/lists/*

RUN groupadd -r symphony \
    && useradd -r -g symphony -u 1001 -m symphony

COPY --from=rust-builder /app/target/release/web-platform /usr/local/bin/web-platform
COPY --from=rust-builder /app/target/release/symphony-platform /usr/local/bin/symphony-platform
COPY --from=frontend-builder /app/dist /srv/frontend

RUN mkdir -p /data /workspaces \
    && chown -R symphony:symphony /data /workspaces

ENV DATABASE_URL=/data/symphony.db
ENV SYMPHONY_BIN=/usr/local/bin/symphony-platform
ENV SYMPHONY_WORKSPACE_ROOT=/workspaces
ENV STATIC_DIR=/srv/frontend
ENV SERVER_HOST=0.0.0.0
ENV SERVER_PORT=3000
ENV RUST_LOG=web_platform=info

USER symphony
VOLUME ["/data", "/workspaces"]
EXPOSE 3000

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -sf http://localhost:3000/health || exit 1

ENTRYPOINT ["web-platform"]
