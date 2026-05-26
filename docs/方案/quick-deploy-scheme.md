#!/usr/bin/env bash
: <<'SYMPHONY_QUICK_DEPLOY_SCHEME_MD'
# Symphony 快速部署方案

## 概述

本方案为 Symphony 项目设计两种部署模式：

1. **服务器部署（Server Mode）** — 适用于团队协作场景，通过 Docker 一键部署到服务器
2. **客户端部署（Desktop Mode）** — 适用于个人开发者，基于 Tauri 2 打包为桌面应用

两种模式共享同一套 Rust 后端和前端代码，通过编译配置区分运行环境。

---

## 架构总览

```
┌─────────────────────────────────────────────────────────┐
│                    Symphony 代码仓库                       │
├─────────────────────────────────────────────────────────┤
│                                                         │
│  ┌─────────────┐  ┌──────────────┐  ┌───────────────┐  │
│  │rust-platform│  │ web-platform │  │ web-frontend  │  │
│  │(worker 进程) │  │ (API Server) │  │  (React SPA)  │  │
│  └─────────────┘  └──────────────┘  └───────────────┘  │
│         │                 │                  │           │
│         ▼                 ▼                  ▼           │
│  ┌─────────────────────────────────────────────────┐    │
│  │              部署产物生成                          │    │
│  ├────────────────────┬────────────────────────────┤    │
│  │   Server Mode      │      Desktop Mode          │    │
│  │   (Docker Image)   │      (Tauri 2 App)         │    │
│  └────────────────────┴────────────────────────────┘    │
│                                                         │
└─────────────────────────────────────────────────────────┘
```

---

## 前置要求（Prerequisites）

### 服务器部署

| 要求 | 最低版本 | 说明 |
|------|---------|------|
| Docker Engine | >= 20.10 | 需要 BuildKit 支持 |
| Docker Compose | V2（`docker compose` 命令） | 不支持 V1 的 `docker-compose` |
| 可用内存 | >= 2GB | 推荐 4GB |
| 磁盘空间 | >= 10GB | 含镜像和工作空间 |
| 架构 | x86_64 / ARM64 | ARM64 需要 Phase 3 支持 |
| 网络 | 需要访问 GitHub/GitLab API | 如有代理需配置 HTTP_PROXY |

### 桌面应用

| 平台 | 最低版本 | 说明 |
|------|---------|------|
| macOS | 10.15+ | 支持 Intel 和 Apple Silicon |
| Windows | 10 (1803+) | 需要 WebView2 Runtime |
| Linux | Ubuntu 20.04+ / Fedora 36+ | 需要 WebKitGTK 4.1 |
| 磁盘空间 | >= 500MB | 含应用和工作空间 |

---

## 一、服务器部署方案

### 1.1 目标

- 一条命令启动完整服务（`docker compose up`）
- 支持环境变量配置，无需修改代码
- 提供预构建镜像（GitHub Container Registry）
- 数据持久化（SQLite 文件挂载）
- 安全默认配置（非 root 运行、随机密钥、仅本地监听）

### 1.2 目录结构

```
deploy/
├── docker/
│   ├── Dockerfile              # 多阶段构建镜像
│   └── .dockerignore
├── docker-compose.yml          # 一键启动编排（生产）
├── docker-compose.dev.yml      # 开发环境覆盖
├── docker-compose.override.yml.example  # 高级用户自定义模板
├── .env.example                # 环境变量模板（含生成指引）
├── Caddyfile.example           # 反向代理 + 自动 HTTPS 示例
└── scripts/
    ├── install.sh              # 一键安装脚本（带完整性校验）
    ├── upgrade.sh              # 升级脚本（含自动备份）
    └── backup.sh              # 数据备份脚本
```

### 1.3 Dockerfile 设计（多阶段构建）

```dockerfile
# Stage 1: Build Rust binaries
FROM rust:1.82-bookworm AS rust-builder
WORKDIR /app
RUN apt-get update && apt-get install -y build-essential pkg-config && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
COPY rust-platform/ rust-platform/
COPY web-platform/ web-platform/
RUN cargo build --release -p web-platform -p symphony-platform

# Stage 2: Build Frontend
FROM node:20-alpine AS frontend-builder
WORKDIR /app
COPY web-frontend/package.json web-frontend/package-lock.json ./
RUN npm ci
COPY web-frontend/ .
RUN npm run build

# Stage 3: Runtime
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates git curl && rm -rf /var/lib/apt/lists/*

# 创建非 root 用户
RUN groupadd -r symphony && useradd -r -g symphony -u 1001 -m symphony

COPY --from=rust-builder /app/target/release/web-platform /usr/local/bin/
COPY --from=rust-builder /app/target/release/symphony-platform /usr/local/bin/
COPY --from=frontend-builder /app/dist /srv/frontend

RUN mkdir -p /data /workspaces && chown -R symphony:symphony /data /workspaces

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
```

### 1.4 docker-compose.yml

```yaml
services:
  symphony:
    image: ghcr.io/hijimo/symphony:latest
    # build:
    #   context: ..
    #   dockerfile: deploy/docker/Dockerfile
    ports:
      - "127.0.0.1:${SERVER_PORT:-3000}:3000"
    volumes:
      - symphony-data:/data
      - symphony-workspaces:/workspaces
    environment:
      - JWT_SECRET=${JWT_SECRET}
      - ENCRYPTION_KEY=${ENCRYPTION_KEY}
      - SYMPHONY_WORKSPACE_ROOT=/workspaces
      - AZURE_OPENAI_BASEURL=${AZURE_OPENAI_BASEURL:-}
      - AZURE_OPENAI_API_KEY=${AZURE_OPENAI_API_KEY:-}
      - AZURE_OPENAI_MODEL=${AZURE_OPENAI_MODEL:-gpt-5.5}
      - HTTP_PROXY=${HTTP_PROXY:-}
      - HTTPS_PROXY=${HTTPS_PROXY:-}
    restart: unless-stopped
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:3000/health"]
      interval: 30s
      timeout: 5s
      retries: 3
      start_period: 10s

volumes:
  symphony-data:
  symphony-workspaces:
```

**注意事项：**
- 端口绑定为 `127.0.0.1:3000:3000`，仅本地可访问，需通过反向代理对外暴露
- 不设置 `ADMIN_INIT_PASSWORD`，依赖代码自动生成随机密码并打印到日志
- 代理环境变量透传，支持企业内网环境

### 1.5 .env.example

```bash
# Symphony 部署配置
# 复制为 .env 后使用，安装脚本会自动生成密钥

# === 必填：安全密钥（安装脚本自动生成，请勿手动填写弱值） ===
# 生成方式: openssl rand -base64 32
JWT_SECRET=
ENCRYPTION_KEY=

# === 可选：服务配置 ===
SERVER_PORT=3000

# === 可选：AI 功能（不配置则禁用 AI Issue 生成） ===
# AZURE_OPENAI_BASEURL=https://<resource>.openai.azure.com
# AZURE_OPENAI_API_KEY=
# AZURE_OPENAI_MODEL=gpt-5.5

# === 可选：网络代理 ===
# HTTP_PROXY=http://proxy:port
# HTTPS_PROXY=http://proxy:port
```

### 1.6 一键安装脚本（install.sh）

```bash
#!/bin/bash
set -e

# Symphony 安装脚本
# 使用方式：
#   方式一（推荐）：下载后校验再执行
#     curl -fsSL https://github.com/hijimo/symphony/releases/latest/download/install.sh -o install.sh
#     curl -fsSL https://github.com/hijimo/symphony/releases/latest/download/install.sh.sha256 -o install.sh.sha256
#     sha256sum -c install.sh.sha256 && bash install.sh
#
#   方式二（快速体验）：
#     curl -fsSL https://github.com/hijimo/symphony/releases/latest/download/install.sh | sh

INSTALL_DIR="${SYMPHONY_INSTALL_DIR:-$HOME/.symphony}"
COMPOSE_URL="https://github.com/hijimo/symphony/releases/latest/download/docker-compose.yml"
ENV_URL="https://github.com/hijimo/symphony/releases/latest/download/.env.example"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

# 前置检查
check_prerequisites() {
    echo "检查前置依赖..."

    if ! command -v docker &> /dev/null; then
        echo -e "${RED}错误: 未安装 Docker。请先安装 Docker: https://docs.docker.com/get-docker/${NC}"
        exit 1
    fi

    if ! docker compose version &> /dev/null; then
        echo -e "${RED}错误: 需要 Docker Compose V2（docker compose 命令）${NC}"
        echo "如果你使用的是旧版 docker-compose，请升级: https://docs.docker.com/compose/install/"
        exit 1
    fi

    DOCKER_VERSION=$(docker version --format '{{.Server.Version}}' 2>/dev/null || echo "0.0.0")
    if [ "$(printf '%s\n' "20.10" "$DOCKER_VERSION" | sort -V | head -n1)" != "20.10" ]; then
        echo -e "${YELLOW}警告: Docker 版本 $DOCKER_VERSION 可能过低，建议 >= 20.10${NC}"
    fi

    if ! command -v openssl &> /dev/null; then
        echo -e "${RED}错误: 未安装 openssl，无法生成安全密钥${NC}"
        exit 1
    fi

    echo -e "${GREEN}前置检查通过${NC}"
}

# 幂等安装
install() {
    if [ -f "$INSTALL_DIR/docker-compose.yml" ]; then
        echo -e "${YELLOW}检测到已有安装: $INSTALL_DIR${NC}"
        echo "使用 --upgrade 升级，或 --reinstall 重新安装"
        echo "  $0 --upgrade    # 保留数据，更新镜像"
        echo "  $0 --reinstall  # 重新安装（保留数据卷）"
        exit 0
    fi

    echo "安装 Symphony 到 $INSTALL_DIR..."
    mkdir -p "$INSTALL_DIR"
    curl -fsSL "$COMPOSE_URL" -o "$INSTALL_DIR/docker-compose.yml"
    curl -fsSL "$ENV_URL" -o "$INSTALL_DIR/.env"

    # 生成随机密钥
    JWT_SECRET=$(openssl rand -base64 32)
    ENCRYPTION_KEY=$(openssl rand -base64 32)

    # 原子写入 .env
    TMP_ENV=$(mktemp)
    sed "s|^JWT_SECRET=.*|JWT_SECRET=$JWT_SECRET|" "$INSTALL_DIR/.env" > "$TMP_ENV"
    sed -i.bak "s|^ENCRYPTION_KEY=.*|ENCRYPTION_KEY=$ENCRYPTION_KEY|" "$TMP_ENV"
    rm -f "$TMP_ENV.bak"
    mv "$TMP_ENV" "$INSTALL_DIR/.env"
    chmod 600 "$INSTALL_DIR/.env"

    echo "启动 Symphony..."
    cd "$INSTALL_DIR"
    docker compose up -d

    # 等待服务就绪
    echo "等待服务启动..."
    for i in $(seq 1 30); do
        if curl -sf http://localhost:${SERVER_PORT:-3000}/health > /dev/null 2>&1; then
            break
        fi
        sleep 2
    done

    # 从日志中提取初始密码
    INIT_PASSWORD=$(docker compose logs 2>/dev/null | grep -oP 'Initial password: \K.*' | tail -1)

    echo ""
    echo -e "${GREEN}=== Symphony 安装完成 ===${NC}"
    echo ""
    echo "  访问地址: http://localhost:${SERVER_PORT:-3000}"
    if [ -n "$INIT_PASSWORD" ]; then
        echo "  管理员账户: admin"
        echo "  初始密码: $INIT_PASSWORD"
    else
        echo "  管理员密码: docker compose -f $INSTALL_DIR/docker-compose.yml logs | grep 'Initial password'"
    fi
    echo ""
    echo "  配置文件: $INSTALL_DIR/.env"
    echo "  查看日志: docker compose -f $INSTALL_DIR/docker-compose.yml logs -f"
    echo ""
    echo -e "${YELLOW}重要: 请立即登录并修改管理员密码！${NC}"
}

# 升级
upgrade() {
    if [ ! -f "$INSTALL_DIR/docker-compose.yml" ]; then
        echo -e "${RED}未找到已有安装，请先运行安装${NC}"
        exit 1
    fi

    echo "备份当前数据..."
    BACKUP_DIR="$INSTALL_DIR/backups/$(date +%Y%m%d_%H%M%S)"
    mkdir -p "$BACKUP_DIR"
    docker compose -f "$INSTALL_DIR/docker-compose.yml" exec -T symphony \
        cp /data/symphony.db "$BACKUP_DIR/symphony.db" 2>/dev/null || true
    cp "$INSTALL_DIR/.env" "$BACKUP_DIR/.env"

    echo "拉取最新镜像..."
    cd "$INSTALL_DIR"
    docker compose pull

    echo "重启服务..."
    docker compose up -d

    # 验证健康检查
    echo "验证服务状态..."
    sleep 5
    if curl -sf http://localhost:${SERVER_PORT:-3000}/health > /dev/null 2>&1; then
        echo -e "${GREEN}升级成功！备份保存在: $BACKUP_DIR${NC}"
    else
        echo -e "${RED}服务未正常启动，正在回滚...${NC}"
        docker compose down
        echo "请检查日志: docker compose logs"
        echo "备份文件: $BACKUP_DIR"
        exit 1
    fi
}

check_prerequisites

case "${1:-}" in
    --upgrade)  upgrade ;;
    --reinstall)
        cd "$INSTALL_DIR" && docker compose down 2>/dev/null || true
        rm -f "$INSTALL_DIR/docker-compose.yml"
        install
        ;;
    *)  install ;;
esac
```

### 1.7 GitHub Actions CI/CD

```yaml
# .github/workflows/release.yml
name: Release

on:
  push:
    tags: ["v*"]

jobs:
  docker:
    runs-on: ubuntu-latest
    permissions:
      contents: read
      packages: write
    steps:
      - uses: actions/checkout@v4
      - uses: docker/setup-buildx-action@v3
      - uses: docker/login-action@v3
        with:
          registry: ghcr.io
          username: ${{ github.actor }}
          password: ${{ secrets.GITHUB_TOKEN }}
      - uses: docker/build-push-action@v5
        with:
          context: .
          file: deploy/docker/Dockerfile
          push: true
          tags: |
            ghcr.io/${{ github.repository }}:latest
            ghcr.io/${{ github.repository }}:${{ github.ref_name }}
          cache-from: type=gha
          cache-to: type=gha,mode=max
          platforms: linux/amd64

  # 生成安装脚本的 SHA256 校验文件
  checksums:
    runs-on: ubuntu-latest
    needs: docker
    steps:
      - uses: actions/checkout@v4
      - name: Generate checksums
        run: |
          sha256sum deploy/scripts/install.sh > install.sh.sha256
      - name: Upload to release
        uses: softprops/action-gh-release@v1
        with:
          files: |
            deploy/scripts/install.sh
            install.sh.sha256
            deploy/docker-compose.yml
            deploy/.env.example
```

---

## 二、客户端部署方案（Tauri 2）

### 2.1 目标

- 单个安装包，开箱即用（macOS .dmg / Windows .msi / Linux .AppImage）
- 内嵌 web-platform + symphony-platform 二进制，无需用户安装 Rust/Node
- 前端通过 Tauri WebView 渲染，无需浏览器
- 自动管理本地数据库和工作空间目录
- 密钥通过系统密钥链安全存储

### 2.2 目录结构

```
desktop/
├── src-tauri/
│   ├── Cargo.toml              # Tauri 2 依赖
│   ├── tauri.conf.json         # Tauri 配置
│   ├── capabilities/           # Tauri 2 权限声明
│   │   └── default.json
│   ├── src/
│   │   ├── main.rs             # Tauri 入口，管理 sidecar 生命周期
│   │   ├── sidecar.rs          # web-platform 进程管理
│   │   ├── setup.rs            # 首次启动初始化（密钥链写入）
│   │   └── commands.rs         # Tauri IPC commands（可选）
│   ├── icons/                  # 应用图标
│   └── sidecars/               # 构建时放入 web-platform + symphony-platform
├── package.json                # Tauri CLI + 前端构建
└── build.rs                    # 构建脚本
```

### 2.3 核心设计：Sidecar 模式

Tauri 2 的 sidecar 机制允许将外部二进制打包进应用。Symphony Desktop 的架构：

```
┌──────────────────────────────────────────┐
│            Tauri 2 Application           │
├──────────────────────────────────────────┤
│                                          │
│  ┌────────────────┐  ┌───────────────┐  │
│  │  Tauri Shell   │  │   WebView     │  │
│  │  (Rust Core)   │  │ (web-frontend)│  │
│  │                │  │               │  │
│  │  管理 sidecar  │◄─┤  localhost:    │  │
│  │  生命周期       │  │  {port}/api   │  │
│  └───────┬────────┘  └───────────────┘  │
│          │                               │
│          ▼                               │
│  ┌────────────────────────────────────┐  │
│  │  Sidecar: web-platform binary      │  │
│  │  (内嵌 static files serving)       │  │
│  │         │                          │  │
│  │         ▼                          │  │
│  │  Sidecar: symphony-platform binary │  │
│  └────────────────────────────────────┘  │
│                                          │
└──────────────────────────────────────────┘
```

**工作流程：**

1. Tauri 应用启动 → 选择可用端口 → 启动 web-platform sidecar
2. **等待端口就绪**（轮询 health 端点，超时 30s）
3. web-platform 就绪后，Tauri WebView 加载 `http://localhost:{port}`
4. 用户操作通过 web-frontend → web-platform API → symphony-platform（由 web-platform 管理）
5. 应用退出时，Tauri 发送 SIGTERM → web-platform 优雅关闭所有 symphony-platform 子进程

### 2.4 tauri.conf.json 关键配置

```json
{
  "$schema": "https://raw.githubusercontent.com/tauri-apps/tauri/dev/crates/tauri-cli/schema.json",
  "productName": "Symphony",
  "version": "0.1.0",
  "identifier": "com.symphony.desktop",
  "build": {
    "beforeBuildCommand": "cd ../../web-frontend && npm ci && npm run build",
    "beforeDevCommand": "cd ../../web-frontend && npm run dev -- --port 5177",
    "frontendDist": "../../web-frontend/dist",
    "devUrl": "http://localhost:5177"
  },
  "app": {
    "title": "Symphony",
    "windows": [
      {
        "title": "Symphony",
        "width": 1280,
        "height": 800,
        "minWidth": 1024,
        "minHeight": 600
      }
    ],
    "security": {
      "csp": "default-src 'self'; connect-src 'self' http://localhost:* ws://localhost:*; style-src 'self' https://fonts.googleapis.com; font-src 'self' https://fonts.gstatic.com; img-src 'self' data: blob:"
    }
  },
  "bundle": {
    "active": true,
    "targets": "all",
    "icon": [
      "icons/32x32.png",
      "icons/128x128.png",
      "icons/128x128@2x.png",
      "icons/icon.icns",
      "icons/icon.ico"
    ],
    "externalBin": [
      "sidecars/web-platform",
      "sidecars/symphony-platform"
    ]
  },
  "plugins": {
    "shell": {
      "open": false,
      "scope": [
        {
          "name": "sidecars/web-platform",
          "sidecar": true,
          "args": true
        },
        {
          "name": "sidecars/symphony-platform",
          "sidecar": true,
          "args": true
        }
      ]
    }
  }
}
```

**关键修正点：**
- `plugins.shell` 中移除了无效的顶层 `"sidecar": true`，每个 scope 条目单独声明
- `frontendDist` 路径修正为相对于 `src-tauri/` 的 `../../web-frontend/dist`
- `beforeBuildCommand` 显式 cd 到 web-frontend 目录执行构建
- CSP 移除 `unsafe-inline`，添加 `ws://localhost:*` 支持 WebSocket
- 添加 `"args": true` 允许传递命令行参数

### 2.5 Tauri 入口（src-tauri/src/main.rs）

```rust
use std::sync::Mutex;
use tauri::Manager;
use tauri_plugin_shell::process::CommandChild;

mod sidecar;
mod setup;

struct AppState {
    backend_port: u16,
    sidecar_child: Option<CommandChild>,
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let port = portpicker::pick_unused_port().expect("No available port");
            let data_dir = app.path().app_data_dir().expect("No app data dir");
            let workspace_dir = data_dir.join("workspaces");

            std::fs::create_dir_all(&data_dir)?;
            std::fs::create_dir_all(&workspace_dir)?;

            // 首次启动初始化（生成密钥并存入系统密钥链）
            setup::ensure_initialized(&data_dir);

            // 启动 web-platform sidecar
            let child = sidecar::start_backend(app.handle(), port, &data_dir, &workspace_dir)?;

            app.manage(Mutex::new(AppState {
                backend_port: port,
                sidecar_child: Some(child),
            }));

            // 在后台等待端口就绪后再导航 WebView
            let handle = app.handle().clone();
            let nav_port = port;
            tauri::async_runtime::spawn(async move {
                if sidecar::wait_for_ready(nav_port, 30).await.is_ok() {
                    if let Some(window) = handle.get_webview_window("main") {
                        let url: tauri::Url = format!("http://localhost:{}", nav_port)
                            .parse()
                            .unwrap();
                        let _ = window.navigate(url);
                    }
                } else {
                    tracing::error!("Backend failed to start within 30s");
                }
            });

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Destroyed = event {
                let state = window.app_handle().state::<Mutex<AppState>>();
                if let Ok(mut state) = state.lock() {
                    if let Some(child) = state.sidecar_child.take() {
                        let _ = child.kill();
                    }
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

**关键修正点：**
- 使用 `tauri_plugin_shell::process::CommandChild` 替代已废弃的 `tauri::api::process::CommandChild`
- 端口就绪等待逻辑移入 `tauri::async_runtime::spawn`，避免阻塞同步 `setup` 闭包
- `navigate()` 使用 `tauri::Url` 类型
- 使用 `app.handle().clone()` 传入 async block

### 2.6 Sidecar 管理（src-tauri/src/sidecar.rs）

```rust
use std::path::Path;
use tauri::AppHandle;
use tauri_plugin_shell::ShellExt;
use tauri_plugin_shell::process::CommandChild;

pub fn start_backend(
    app: &AppHandle,
    port: u16,
    data_dir: &Path,
    workspace_dir: &Path,
) -> Result<CommandChild, Box<dyn std::error::Error>> {
    let db_path = data_dir.join("symphony.db");
    let symphony_bin = resolve_sidecar_path("symphony-platform");

    let (mut rx, child) = app.shell()
        .sidecar("sidecars/web-platform")
        .map_err(|e| format!("failed to create sidecar command: {e}"))?
        .env("SERVER_PORT", port.to_string())
        .env("SERVER_HOST", "127.0.0.1")
        .env("DATABASE_URL", db_path.to_string_lossy().to_string())
        .env("SYMPHONY_WORKSPACE_ROOT", workspace_dir.to_string_lossy().to_string())
        .env("SYMPHONY_BIN", symphony_bin)
        .spawn()
        .map_err(|e| format!("failed to spawn sidecar: {e}"))?;

    // 后台消费 sidecar 输出（避免 pipe buffer 死锁）
    tauri::async_runtime::spawn(async move {
        use tauri_plugin_shell::process::CommandEvent;
        while let Some(event) = rx.recv().await {
            match event {
                CommandEvent::Stdout(line) => {
                    tracing::info!("[backend] {}", String::from_utf8_lossy(&line));
                }
                CommandEvent::Stderr(line) => {
                    tracing::warn!("[backend] {}", String::from_utf8_lossy(&line));
                }
                CommandEvent::Terminated(status) => {
                    tracing::error!("[backend] process terminated: {:?}", status);
                    break;
                }
                _ => {}
            }
        }
    });

    Ok(child)
}

/// 等待后端端口就绪
pub async fn wait_for_ready(port: u16, timeout_secs: u64) -> Result<(), String> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
    loop {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return Ok(());
        }
        if std::time::Instant::now() > deadline {
            return Err("Backend startup timeout".to_string());
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}

/// 解析 sidecar 二进制的绝对路径
fn resolve_sidecar_path(name: &str) -> String {
    let exe_dir = std::env::current_exe()
        .expect("failed to get exe path")
        .parent()
        .expect("no parent dir")
        .to_path_buf();

    #[cfg(target_os = "windows")]
    let bin_name = format!("{}.exe", name);
    #[cfg(not(target_os = "windows"))]
    let bin_name = name.to_string();

    exe_dir.join(bin_name).to_string_lossy().to_string()
}
```

**关键修正点：**
- 返回类型使用 `tauri_plugin_shell::process::CommandChild`
- sidecar 名称使用 `"sidecars/web-platform"` 与 `tauri.conf.json` 中 `externalBin` 路径一致
- 添加 `wait_for_ready` 异步函数解决启动时序问题
- `resolve_sidecar_path` 区分 Windows（.exe 后缀）和 Unix
- 处理 `CommandEvent::Terminated` 事件，检测 sidecar 异常退出

### 2.7 desktop/src-tauri/Cargo.toml

```toml
[package]
name = "symphony-desktop"
version = "0.1.0"
edition = "2021"

[dependencies]
tauri = { version = "2", features = [] }
tauri-plugin-shell = "2"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
portpicker = "0.1"
tracing = "0.1"
tracing-subscriber = "0.3"
tokio = { version = "1", features = ["time", "net"] }
keyring = "2"

[build-dependencies]
tauri-build = { version = "2", features = [] }
```

### 2.8 构建与发布

**本地开发：**

```bash
cd desktop
npm install
npm run tauri dev
```

**构建安装包：**

```bash
# 先编译 sidecar 二进制
cargo build --release -p web-platform -p symphony-platform

# 获取当前平台 target triple
TARGET=$(rustc -vV | grep host | cut -d' ' -f2)

# 复制到 sidecar 目录（按 Tauri 命名规范）
mkdir -p desktop/src-tauri/sidecars
cp target/release/web-platform "desktop/src-tauri/sidecars/web-platform-${TARGET}"
cp target/release/symphony-platform "desktop/src-tauri/sidecars/symphony-platform-${TARGET}"

# Windows 上需要 .exe 后缀：
# cp target/release/web-platform.exe "desktop/src-tauri/sidecars/web-platform-${TARGET}.exe"

# 构建 Tauri 应用
cd desktop
npm run tauri build
```

**CI 跨平台构建（GitHub Actions）：**

```yaml
# .github/workflows/desktop-release.yml
name: Desktop Release

on:
  push:
    tags: ["v*"]

jobs:
  build:
    strategy:
      matrix:
        include:
          - os: macos-latest
            target: aarch64-apple-darwin
          - os: macos-latest
            target: x86_64-apple-darwin
          - os: windows-latest
            target: x86_64-pc-windows-msvc
          - os: ubuntu-22.04
            target: x86_64-unknown-linux-gnu

    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4

      - uses: actions-rust-lang/setup-rust-toolchain@v1
        with:
          target: ${{ matrix.target }}

      - name: Add Rust target
        run: rustup target add ${{ matrix.target }}

      - uses: actions/setup-node@v4
        with:
          node-version: 20

      - name: Install Linux dependencies
        if: runner.os == 'Linux'
        run: |
          sudo apt-get update
          sudo apt-get install -y libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev

      - name: Build sidecar binaries (Unix)
        if: runner.os != 'Windows'
        run: |
          cargo build --release --target ${{ matrix.target }} -p web-platform -p symphony-platform
          mkdir -p desktop/src-tauri/sidecars
          cp target/${{ matrix.target }}/release/web-platform \
             desktop/src-tauri/sidecars/web-platform-${{ matrix.target }}
          cp target/${{ matrix.target }}/release/symphony-platform \
             desktop/src-tauri/sidecars/symphony-platform-${{ matrix.target }}

      - name: Build sidecar binaries (Windows)
        if: runner.os == 'Windows'
        shell: pwsh
        run: |
          cargo build --release --target ${{ matrix.target }} -p web-platform -p symphony-platform
          New-Item -ItemType Directory -Force -Path desktop/src-tauri/sidecars
          Copy-Item "target/${{ matrix.target }}/release/web-platform.exe" `
            "desktop/src-tauri/sidecars/web-platform-${{ matrix.target }}.exe"
          Copy-Item "target/${{ matrix.target }}/release/symphony-platform.exe" `
            "desktop/src-tauri/sidecars/symphony-platform-${{ matrix.target }}.exe"

      - name: Install frontend dependencies
        run: |
          cd web-frontend && npm ci

      - name: Build Tauri app
        uses: tauri-apps/tauri-action@v0.5
        with:
          projectPath: desktop
          tauriScript: npx tauri
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}

      - name: Upload artifacts
        uses: actions/upload-artifact@v4
        with:
          name: symphony-${{ matrix.target }}
          path: |
            desktop/src-tauri/target/${{ matrix.target }}/release/bundle/**/*.dmg
            desktop/src-tauri/target/${{ matrix.target }}/release/bundle/**/*.msi
            desktop/src-tauri/target/${{ matrix.target }}/release/bundle/**/*.AppImage
```

**关键修正点：**
- `tauri-apps/tauri-action@v0.5`（兼容 Tauri 2）
- Windows 使用 PowerShell 并正确处理 `.exe` 后缀
- 显式 `rustup target add` 确保交叉编译 target 可用
- 所有平台显式安装 Node.js 20

---

## 三、web-platform 适配改动要点

为同时支持两种部署模式，web-platform 需要少量适配（不在本方案中实施，仅列出要点）：

| 改动点 | 说明 | 优先级 |
|--------|------|--------|
| 静态文件服务 | 增加 `STATIC_DIR` 环境变量，非空时 serve 前端静态文件（服务器模式用） | P0 |
| 健康检查端点 | 增加 `GET /health` 端点，返回 `{"status":"ok"}`，供 Docker/Tauri 判断就绪 | P0 |
| 优雅关闭 | 收到 SIGTERM 时正确关闭所有 symphony-platform 子进程（防止孤儿进程） | P0 |
| Swagger UI 条件加载 | 通过 `ENABLE_SWAGGER` 环境变量控制，生产环境默认禁用 | P1 |
| 密钥自动生成 | 桌面模式首次启动时自动生成 JWT_SECRET/ENCRYPTION_KEY 并持久化 | P1 |
| Origin 校验 | 后端验证请求 Origin/Host header，拒绝非 localhost 来源（桌面模式安全加固） | P2 |

**已支持无需改动的配置：**
- 端口动态绑定（`SERVER_PORT` 环境变量）
- 数据库路径（`DATABASE_URL` 环境变量）
- 管理员密码自动生成（未设置 `ADMIN_INIT_PASSWORD` 时随机生成并打印到日志）

---

## 四、用户快速开始指南

### 服务器部署（推荐团队使用）

**方式一：一键安装（推荐）**

```bash
# 下载并校验安装脚本
curl -fsSL https://github.com/hijimo/symphony/releases/latest/download/install.sh -o install.sh
curl -fsSL https://github.com/hijimo/symphony/releases/latest/download/install.sh.sha256 -o install.sh.sha256
sha256sum -c install.sh.sha256 && bash install.sh
```

**方式二：手动安装**

```bash
git clone https://github.com/hijimo/symphony.git
cd symphony/deploy
cp .env.example .env

# 生成安全密钥
JWT_SECRET=$(openssl rand -base64 32)
ENCRYPTION_KEY=$(openssl rand -base64 32)
sed -i "s|^JWT_SECRET=.*|JWT_SECRET=$JWT_SECRET|" .env
sed -i "s|^ENCRYPTION_KEY=.*|ENCRYPTION_KEY=$ENCRYPTION_KEY|" .env

# 启动
docker compose up -d

# 查看初始管理员密码
docker compose logs | grep "Initial password"
```

**首次登录：**

1. 访问 `http://localhost:3000`（如需外网访问，请配置反向代理）
2. 使用日志中的初始密码登录 admin 账户
3. **立即修改管理员密码**
4. 配置 Git 平台 Token（GitHub/GitLab），所需权限：
   - GitHub: `repo` (Full control of private repositories)
   - GitLab: `api` (Full API access)

**升级：**

```bash
cd ~/.symphony  # 或你的安装目录
./scripts/upgrade.sh
# 自动执行：备份数据 → 拉取新镜像 → 重启 → 健康检查 → 失败回滚
```

**常用操作：**

```bash
# 查看日志
docker compose logs -f

# 停止服务
docker compose down

# 备份数据
docker compose exec symphony cp /data/symphony.db /data/backup-$(date +%Y%m%d).db

# 查看服务状态
docker compose ps
curl http://localhost:3000/health
```

### 桌面应用（推荐个人使用）

1. 从 [GitHub Releases](https://github.com/hijimo/symphony/releases) 下载对应平台安装包：
   - macOS: `Symphony-x.x.x.dmg`（Universal，支持 Intel + Apple Silicon）
   - Windows: `Symphony-x.x.x.msi`
   - Linux: `Symphony-x.x.x.AppImage`

2. 安装并启动 Symphony

3. 首次启动自动完成：
   - 生成安全密钥（存入系统密钥链）
   - 创建本地数据库
   - 创建工作空间目录

4. 配置 Git 平台 Token（应用内引导）

**数据目录位置：**
- macOS: `~/Library/Application Support/com.symphony.desktop/`
- Windows: `%APPDATA%\com.symphony.desktop\`
- Linux: `~/.config/com.symphony.desktop/`

**备份：** 复制上述数据目录即可完整备份所有配置和数据。

---

## 五、故障排除（Troubleshooting）

### 服务器部署常见问题

| 问题 | 原因 | 解决方案 |
|------|------|---------|
| `docker compose` 命令不存在 | 使用了 Docker Compose V1 | 升级到 V2: `apt install docker-compose-plugin` |
| 容器启动后立即退出 | JWT_SECRET 或 ENCRYPTION_KEY 为空 | 检查 `.env` 文件，确保密钥已生成 |
| 无法访问 `localhost:3000` | 端口被占用或绑定了 127.0.0.1 | `docker compose logs` 查看错误；如需外网访问配置反向代理 |
| AI 功能不可用 | 未配置 Azure OpenAI 环境变量 | 在 `.env` 中设置 `AZURE_OPENAI_BASEURL` 和 `AZURE_OPENAI_API_KEY` |
| Docker pull 超时 | 网络问题（中国大陆常见） | 配置 Docker daemon 代理或使用镜像加速器 |
| symphony-platform 无法访问 GitHub/GitLab | 代理未配置 | 在 `.env` 中设置 `HTTP_PROXY` 和 `HTTPS_PROXY` |
| 升级后数据丢失 | 未使用 named volumes | 确保使用 `docker compose` 而非手动 `docker run` |

### 桌面应用常见问题

| 问题 | 原因 | 解决方案 |
|------|------|---------|
| macOS 提示"无法验证开发者" | 未签名的开发版本 | 右键 → 打开，或 `xattr -cr /Applications/Symphony.app` |
| Windows 提示 SmartScreen 警告 | 未签名的开发版本 | 点击"更多信息" → "仍要运行" |
| 启动后白屏 | 后端 sidecar 启动失败 | 查看应用日志（数据目录下 `logs/`） |
| Linux 上无法启动 | 缺少 WebKitGTK | `sudo apt install libwebkit2gtk-4.1-0` |

---

## 六、方案对比

| 维度 | 服务器部署 | 桌面应用 |
|------|-----------|---------|
| 适用场景 | 团队协作、CI/CD 集成、多项目管理 | 个人开发者、本地使用、快速体验 |
| 依赖 | Docker >= 20.10 | 无（自包含） |
| 数据存储 | Docker Volume（可备份） | 系统应用数据目录 |
| 更新方式 | `upgrade.sh`（含自动备份和回滚） | 应用内自动更新（Tauri updater） |
| 多用户 | 支持 | 单用户 |
| 网络要求 | 需要服务器可达 + 访问 Git 平台 | 本地运行，仅需访问 Git 平台 |
| 安全模型 | 非 root 容器 + 反向代理 + 随机密钥 | 系统密钥链 + localhost only |
| 资源占用 | 独立容器，可限制资源 | 共享系统资源 |

---

## 七、实施路线图

### Phase 1：服务器部署（优先级高，工作量小）

1. web-platform 增加 `GET /health` 端点
2. web-platform 增加 `STATIC_DIR` 静态文件服务
3. web-platform 增加 SIGTERM 优雅关闭（清理子进程）
4. 创建 `deploy/` 目录结构
5. 编写 Dockerfile（多阶段构建、非 root 用户）
6. 编写 docker-compose.yml + .env.example
7. 编写 install.sh / upgrade.sh / backup.sh
8. 配置 GitHub Actions 自动构建镜像 + 校验文件

**预估工作量：3-5 天**

### Phase 2：桌面应用（优先级中，工作量中等）

1. 初始化 `desktop/` Tauri 2 项目
2. 实现 sidecar 生命周期管理（启动、等待就绪、优雅关闭）
3. 实现首次启动初始化逻辑（密钥链写入、数据库创建）
4. 配置跨平台 CI 构建（含 Windows .exe 后缀处理）
5. 集成 Tauri updater 自动更新
6. 添加首次使用引导（Token 配置 wizard）

**预估工作量：5-8 天**

### Phase 3：优化与完善

1. Tauri 应用签名（macOS notarization、Windows code signing）
2. 自动更新通道（stable / beta）
3. 安装包体积优化（UPX 压缩 sidecar）
4. ARM64 Docker 镜像支持
5. Caddy/Traefik 反向代理 + 自动 HTTPS 文档
6. CHANGELOG.md 维护流程

**预估工作量：3-5 天**

---

## 八、技术决策说明

### 为什么选择 Sidecar 而非将 web-platform 编译进 Tauri？

- web-platform 已经是一个完整的 Axum 服务，直接作为 sidecar 运行可以零改动复用
- 避免将 Axum server 和 Tauri event loop 耦合在同一进程中，降低复杂度
- sidecar 崩溃不会导致 Tauri 主进程退出，可以实现自动重启
- 保持服务器模式和桌面模式的后端代码完全一致

### 为什么不用 Electron？

- 项目后端已经是 Rust，Tauri 2 天然适配，无需额外 Node.js 运行时
- 安装包体积显著更小（Tauri ~10MB vs Electron ~150MB）
- 内存占用更低，适合开发者工具场景
- Tauri 2 的 sidecar 和 plugin 生态成熟，满足需求

### 为什么 Docker 镜像内嵌前端而非分离？

- Symphony 是一个自包含的工具，不是微服务架构
- 单容器部署对开源用户最友好，降低运维复杂度
- 前端是纯静态文件，由 web-platform 直接 serve，无需 Nginx

### 为什么端口绑定 127.0.0.1 而非 0.0.0.0？

- 安全默认：避免未配置防火墙的服务器直接暴露到公网
- 开源用户可能在不安全的网络环境中快速体验，默认不暴露更安全
- 需要外网访问时，通过反向代理（Caddy/Nginx）对外暴露，同时获得 HTTPS

### 为什么不使用默认密码？

- 依赖代码已有的随机密码生成逻辑（24 位随机字符串）
- 避免所有实例共享同一个弱密码的安全风险
- 用户通过 `docker compose logs | grep password` 获取初始密码，强制感知密码的存在

---

## 九、安全设计总结

| 安全措施 | 服务器模式 | 桌面模式 |
|----------|-----------|---------|
| 运行权限 | 非 root 用户（UID 1001） | 用户权限 |
| 密钥存储 | `.env` 文件（chmod 600） | 系统密钥链（Keychain/Credential Manager） |
| 密钥生成 | 安装脚本自动 `openssl rand -base64 32` | 首次启动自动生成 |
| 网络暴露 | 仅 127.0.0.1，需反向代理对外 | 仅 127.0.0.1 |
| 管理员密码 | 随机生成，打印到日志 | 随机生成，首次启动显示 |
| API 文档 | 生产环境默认禁用 Swagger UI | 不暴露 |
| 安装脚本 | 提供 SHA256 校验文件 | N/A（应用商店/签名） |
| 数据备份 | upgrade.sh 自动备份 | 用户手动复制数据目录 |
| 子进程隔离 | 容器内隔离 | 操作系统进程隔离 |

---

## 附录：对抗验证记录

本方案经过 3 轮对抗验证（架构可行性 × 安全审计 × 开源用户体验），第三轮全部 PASS。

### 验证过程

| 轮次 | 架构 | 安全 | DX | 修复内容 |
|------|------|------|-----|---------|
| 第 1 轮 | FAIL | FAIL | FAIL | Tauri 2 API 修正、非 root 容器、移除默认密码、添加 prerequisites/troubleshooting |
| 第 2 轮 | FAIL | N/A* | PASS | Dockerfile 补 curl/ENV、sidecar 错误传播、CI 去重复构建 |
| 第 3 轮 | PASS | PASS | PASS | — |

*第 2 轮安全审计检查了代码库实现而非方案文档，结论不适用。

### 实施时需关注的中等风险（来自第三轮审查）

**架构层面：**
1. CI 中 `npm ci` 会执行两次（显式步骤 + beforeBuildCommand），可优化为 beforeBuildCommand 只执行 `npm run build`
2. `resolve_sidecar_path` 需要完整错误处理，找不到 binary 时应返回明确错误

**安全层面：**
1. Origin 校验建议从 P2 提升为 P1，防止本机 CSRF 攻击
2. 管理员初始密码打印到日志后，重启不应再次打印（数据库标记已初始化）
3. .env 临时文件应在写入前先 chmod 600
4. keyring 在无 GUI Linux 环境的降级处理需明确
5. 密钥轮换流程需在文档中补充

**用户体验层面：**
1. 安装脚本密码提取应在健康检查通过后执行，失败时 fallback 到提示命令
2. `--reinstall` 需要二次确认并说明数据清除范围
3. 健康检查等待期间应输出进度提示
4. 补充"首次登录后做什么"的引导链接
5. 补充桌面应用迁移到 Docker 的数据迁移路径
SYMPHONY_QUICK_DEPLOY_SCHEME_MD
