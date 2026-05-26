#!/bin/bash
set -euo pipefail

INSTALL_DIR="${SYMPHONY_INSTALL_DIR:-$HOME/.symphony}"
RELEASE_BASE_URL="${SYMPHONY_RELEASE_BASE_URL:-https://github.com/hijimo/symphony/releases/latest/download}"
COMPOSE_URL="$RELEASE_BASE_URL/docker-compose.yml"
ENV_URL="$RELEASE_BASE_URL/.env.example"
INSTALL_SCRIPT_URL="$RELEASE_BASE_URL/install.sh"
INSTALL_SHA_URL="$RELEASE_BASE_URL/install.sh.sha256"
UPGRADE_URL="$RELEASE_BASE_URL/upgrade.sh"
BACKUP_URL="$RELEASE_BASE_URL/backup.sh"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

usage() {
    cat <<EOF
Symphony 安装脚本

Usage:
  $0              安装 Symphony
  $0 --upgrade    保留数据并升级镜像
  $0 --reinstall  重新安装 compose 文件（保留 Docker 数据卷）

推荐安装方式：
  curl -fsSL $INSTALL_SCRIPT_URL -o install.sh
  curl -fsSL $INSTALL_SHA_URL -o install.sh.sha256
  sha256sum -c install.sh.sha256 && bash install.sh
EOF
}

check_prerequisites() {
    echo "检查前置依赖..."

    if ! command -v docker >/dev/null 2>&1; then
        echo -e "${RED}错误: 未安装 Docker。请先安装 Docker: https://docs.docker.com/get-docker/${NC}" >&2
        exit 1
    fi

    if ! docker compose version >/dev/null 2>&1; then
        echo -e "${RED}错误: 需要 Docker Compose V2（docker compose 命令）${NC}" >&2
        echo "如果你使用的是旧版 docker-compose，请升级: https://docs.docker.com/compose/install/" >&2
        exit 1
    fi

    local docker_version
    docker_version="$(docker version --format '{{.Server.Version}}' 2>/dev/null || echo "0.0.0")"
    if [ "$(printf '%s\n' "20.10" "$docker_version" | sort -V | head -n1)" != "20.10" ]; then
        echo -e "${YELLOW}警告: Docker 版本 $docker_version 可能过低，建议 >= 20.10${NC}"
    fi

    if ! command -v openssl >/dev/null 2>&1; then
        echo -e "${RED}错误: 未安装 openssl，无法生成安全密钥${NC}" >&2
        exit 1
    fi

    if ! command -v curl >/dev/null 2>&1; then
        echo -e "${RED}错误: 未安装 curl，无法下载安装文件${NC}" >&2
        exit 1
    fi

    echo -e "${GREEN}前置检查通过${NC}"
}

download_file() {
    local url="$1"
    local output="$2"
    curl -fsSL "$url" -o "$output"
}

write_env_with_secrets() {
    local source_env="$1"
    local target_env="$2"
    local jwt_secret="$3"
    local encryption_key="$4"
    local tmp_env

    tmp_env="$(mktemp)"
    chmod 600 "$tmp_env"
    awk -v jwt="$jwt_secret" -v enc="$encryption_key" '
        /^JWT_SECRET=/ { print "JWT_SECRET=" jwt; next }
        /^ENCRYPTION_KEY=/ { print "ENCRYPTION_KEY=" enc; next }
        { print }
    ' "$source_env" > "$tmp_env"
    mv "$tmp_env" "$target_env"
    chmod 600 "$target_env"
}

wait_for_health() {
    local port="${SERVER_PORT:-3000}"
    echo "等待服务启动..."
    for i in $(seq 1 30); do
        if curl -sf "http://localhost:${port}/health" >/dev/null 2>&1; then
            echo -e "${GREEN}服务健康检查通过${NC}"
            return 0
        fi
        printf '  health check %s/30 pending...\n' "$i"
        sleep 2
    done

    echo -e "${RED}错误: 服务未在 60 秒内通过健康检查。请运行 docker compose logs 查看详情。${NC}" >&2
    return 1
}

install() {
    if [ -f "$INSTALL_DIR/docker-compose.yml" ]; then
        echo -e "${YELLOW}检测到已有安装: $INSTALL_DIR${NC}"
        echo "使用 --upgrade 升级，或 --reinstall 重新安装"
        echo "  $0 --upgrade    # 保留数据，更新镜像"
        echo "  $0 --reinstall  # 重新安装（保留数据卷）"
        exit 0
    fi

    echo "安装 Symphony 到 $INSTALL_DIR..."
    mkdir -p "$INSTALL_DIR/scripts"

    download_file "$COMPOSE_URL" "$INSTALL_DIR/docker-compose.yml"
    download_file "$ENV_URL" "$INSTALL_DIR/.env.example"
    download_file "$UPGRADE_URL" "$INSTALL_DIR/scripts/upgrade.sh"
    download_file "$BACKUP_URL" "$INSTALL_DIR/scripts/backup.sh"
    chmod +x "$INSTALL_DIR/scripts/upgrade.sh" "$INSTALL_DIR/scripts/backup.sh"

    local jwt_secret
    local encryption_key
    jwt_secret="$(openssl rand -base64 32)"
    encryption_key="$(openssl rand -base64 32)"
    write_env_with_secrets "$INSTALL_DIR/.env.example" "$INSTALL_DIR/.env" "$jwt_secret" "$encryption_key"

    echo "启动 Symphony..."
    cd "$INSTALL_DIR"
    docker compose up -d
    wait_for_health

    local init_password
    init_password="$(docker compose logs 2>/dev/null | sed -n 's/.*Initial password: //p' | tail -1)"

    echo ""
    echo -e "${GREEN}=== Symphony 安装完成 ===${NC}"
    echo ""
    echo "  访问地址: http://localhost:${SERVER_PORT:-3000}"
    if [ -n "$init_password" ]; then
        echo "  管理员账户: admin"
        echo "  初始密码: $init_password"
    else
        echo "  管理员密码: docker compose -f $INSTALL_DIR/docker-compose.yml logs | grep 'Initial password'"
    fi
    echo ""
    echo "  配置文件: $INSTALL_DIR/.env"
    echo "  查看日志: docker compose -f $INSTALL_DIR/docker-compose.yml logs -f"
    echo ""
    echo -e "${YELLOW}重要: 请立即登录并修改管理员密码！${NC}"
}

upgrade() {
    "$INSTALL_DIR/scripts/upgrade.sh"
}

reinstall() {
    if [ ! -f "$INSTALL_DIR/docker-compose.yml" ]; then
        install
        return
    fi

    echo -e "${YELLOW}--reinstall 会替换 compose 文件和脚本，但保留 Docker 数据卷。${NC}"
    if [ "${SYMPHONY_REINSTALL_CONFIRM:-}" != "yes" ]; then
        echo "如需无人值守执行，请设置 SYMPHONY_REINSTALL_CONFIRM=yes 后重试。" >&2
        exit 1
    fi

    cd "$INSTALL_DIR"
    docker compose down 2>/dev/null || true
    rm -f "$INSTALL_DIR/docker-compose.yml"
    install
}

check_prerequisites

case "${1:-}" in
    --help|-h) usage ;;
    --upgrade) upgrade ;;
    --reinstall) reinstall ;;
    "") install ;;
    *)
        usage >&2
        exit 1
        ;;
esac
