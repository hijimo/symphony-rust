# syntax=docker/dockerfile:1.7

FROM rust:1.98-bookworm AS rust-builder
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

# Symphony 运行时依赖的外部 CLI 版本（可用 --build-arg 覆盖）
ARG GH_VERSION=2.101.0
ARG GLAB_VERSION=1.119.0
ARG CODEX_VERSION=0.159.0
ARG TARGETARCH

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl git libssl3 jq \
    && rm -rf /var/lib/apt/lists/*

# codex：rust-platform 通过 `codex app-server` 拉起编码代理
# gh / glab：WORKFLOW.md.github / WORKFLOW.md.gitlab 的提示词依赖这两个 CLI 做 Issue/PR 状态流转
RUN set -eux; \
    arch="${TARGETARCH:-$(dpkg --print-architecture)}"; \
    case "$arch" in \
        arm64) codex_triple=aarch64-unknown-linux-musl ;; \
        amd64) codex_triple=x86_64-unknown-linux-musl ;; \
        *) echo "unsupported arch: $arch" >&2; exit 1 ;; \
    esac; \
    curl -fsSL "https://github.com/cli/cli/releases/download/v${GH_VERSION}/gh_${GH_VERSION}_linux_${arch}.tar.gz" -o /tmp/gh.tgz; \
    tar -xzf /tmp/gh.tgz -C /tmp "gh_${GH_VERSION}_linux_${arch}/bin/gh"; \
    install -m 0755 "/tmp/gh_${GH_VERSION}_linux_${arch}/bin/gh" /usr/local/bin/gh; \
    curl -fsSL "https://gitlab.com/gitlab-org/cli/-/releases/v${GLAB_VERSION}/downloads/glab_${GLAB_VERSION}_linux_${arch}.tar.gz" -o /tmp/glab.tgz; \
    tar -xzf /tmp/glab.tgz -C /tmp bin/glab; \
    install -m 0755 /tmp/bin/glab /usr/local/bin/glab; \
    curl -fsSL "https://github.com/openai/codex/releases/download/rust-v${CODEX_VERSION}/codex-${codex_triple}.tar.gz" -o /tmp/codex.tgz; \
    tar -xzf /tmp/codex.tgz -C /tmp "codex-${codex_triple}"; \
    install -m 0755 "/tmp/codex-${codex_triple}" /usr/local/bin/codex; \
    rm -rf /tmp/gh.tgz /tmp/glab.tgz /tmp/codex.tgz "/tmp/gh_${GH_VERSION}_linux_${arch}" /tmp/bin; \
    gh --version; \
    glab --version; \
    codex --version

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
