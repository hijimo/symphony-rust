#!/bin/bash

PROJECT_ROOT="$(cd "$(dirname "$0")" && pwd)"

# 可选：本地覆盖配置。不要提交包含真实密钥的 .env.local。
if [ -f "$PROJECT_ROOT/.env.local" ]; then
    set -a
    # shellcheck disable=SC1091
    source "$PROJECT_ROOT/.env.local"
    set +a
elif [ -f "$PROJECT_ROOT/.env" ]; then
    set -a
    # shellcheck disable=SC1091
    source "$PROJECT_ROOT/.env"
    set +a
fi

# 环境变量（开发用，生产环境请使用 .env 文件或密钥管理服务）
export JWT_SECRET="${JWT_SECRET:-dev-secret-key-at-least-32-chars-long}"
export ENCRYPTION_KEY="${ENCRYPTION_KEY:-MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=}"
export DATABASE_URL="${DATABASE_URL:-$PROJECT_ROOT/data.db}"
export SERVER_HOST="${SERVER_HOST:-0.0.0.0}"
export SERVER_PORT="${SERVER_PORT:-3000}"
export ADMIN_INIT_PASSWORD="${ADMIN_INIT_PASSWORD:-admin123}"
export RUST_LOG="${RUST_LOG:-web_platform=debug,tower_http=debug}"
export SYMPHONY_BIN="${SYMPHONY_BIN:-$PROJECT_ROOT/target/debug/symphony-platform}"
export SYMPHONY_WORKSPACE_ROOT="${SYMPHONY_WORKSPACE_ROOT:-$PROJECT_ROOT/workspaces}"

# AI Issue 生成配置。AZURE_OPENAI_BASEURL 和 AZURE_OPENAI_API_KEY 都存在时才启用。
# Azure 示例：
#   AZURE_OPENAI_BASEURL=https://<resource>.openai.azure.com
#   AZURE_OPENAI_API_KEY=<your-key>
#   AZURE_OPENAI_MODEL=<deployment-name>
#   AI_MODEL_FAMILY=gpt5
# OpenAI-compatible /v1 示例：
#   AZURE_OPENAI_BASEURL=https://api.openai.com/v1
#   AZURE_OPENAI_API_KEY=<your-key>
#   AZURE_OPENAI_MODEL=gpt-5.5
export AZURE_OPENAI_MODEL="${AZURE_OPENAI_MODEL:-gpt-5.5}"
export AI_MAX_TOKENS="${AI_MAX_TOKENS:-4096}"
export AI_RATE_LIMIT_PER_MINUTE="${AI_RATE_LIMIT_PER_MINUTE:-10}"
export AI_GLOBAL_RATE_LIMIT_PER_MINUTE="${AI_GLOBAL_RATE_LIMIT_PER_MINUTE:-30}"
if [ -n "${AI_MODEL_FAMILY:-}" ]; then
    export AI_MODEL_FAMILY
fi
if [ -n "${AZURE_OPENAI_BASEURL:-}" ] && [ -n "${AZURE_OPENAI_API_KEY:-}" ]; then
    export AZURE_OPENAI_BASEURL
    export AZURE_OPENAI_API_KEY
fi

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

cleanup() {
    echo ""
    echo -e "${YELLOW}Stopping services...${NC}"
    if [ -n "$BACKEND_PID" ] && kill -0 "$BACKEND_PID" 2>/dev/null; then
        kill "$BACKEND_PID"
        wait "$BACKEND_PID" 2>/dev/null
        echo -e "${GREEN}Backend stopped${NC}"
    fi
    if [ -n "$FRONTEND_PID" ] && kill -0 "$FRONTEND_PID" 2>/dev/null; then
        kill "$FRONTEND_PID"
        wait "$FRONTEND_PID" 2>/dev/null
        echo -e "${GREEN}Frontend stopped${NC}"
    fi
    exit 0
}

trap cleanup SIGINT SIGTERM

echo -e "${GREEN}=== Symphony Dev Environment ===${NC}"
echo -e "Project: $PROJECT_ROOT"
if [ -n "${AZURE_OPENAI_BASEURL:-}" ] && [ -n "${AZURE_OPENAI_API_KEY:-}" ]; then
    echo -e "AI:      enabled (${AZURE_OPENAI_MODEL}, family=${AI_MODEL_FAMILY:-auto})"
else
    echo -e "AI:      disabled (set AZURE_OPENAI_BASEURL and AZURE_OPENAI_API_KEY to enable)"
fi
echo ""

# 编译后端
echo -e "${YELLOW}Building backend...${NC}"
cargo build -p web-platform 2>&1
if [ $? -ne 0 ]; then
    echo -e "${RED}Backend build failed${NC}"
    exit 1
fi
echo -e "${GREEN}Backend build OK${NC}"

# 启动后端
echo -e "${YELLOW}Starting backend on :${SERVER_PORT}...${NC}"
cargo run -p web-platform &
BACKEND_PID=$!
sleep 2

if ! kill -0 "$BACKEND_PID" 2>/dev/null; then
    echo -e "${RED}Backend failed to start${NC}"
    exit 1
fi
echo -e "${GREEN}Backend running (PID: $BACKEND_PID)${NC}"

# 启动前端
echo -e "${YELLOW}Starting frontend...${NC}"
cd "$PROJECT_ROOT/web-frontend"
npm run dev -- --port 5177 &
FRONTEND_PID=$!
cd "$PROJECT_ROOT"
sleep 2

if ! kill -0 "$FRONTEND_PID" 2>/dev/null; then
    echo -e "${RED}Frontend failed to start${NC}"
    cleanup
    exit 1
fi
echo -e "${GREEN}Frontend running (PID: $FRONTEND_PID)${NC}"

echo ""
echo -e "${GREEN}=== Dev environment ready ===${NC}"
echo -e "  Backend:  http://localhost:${SERVER_PORT}"
echo -e "  Frontend: http://localhost:5177"
echo -e "  API Docs: http://localhost:${SERVER_PORT}/swagger-ui"
echo ""
echo -e "${YELLOW}Press Ctrl+C to stop all services${NC}"

wait
