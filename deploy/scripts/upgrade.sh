#!/bin/bash
set -euo pipefail

INSTALL_DIR="${SYMPHONY_INSTALL_DIR:-$HOME/.symphony}"
SERVER_PORT="${SERVER_PORT:-3000}"

RED='\033[0;31m'
GREEN='\033[0;32m'
NC='\033[0m'

if [ ! -f "$INSTALL_DIR/docker-compose.yml" ]; then
    echo -e "${RED}未找到已有安装，请先运行 install.sh${NC}" >&2
    exit 1
fi

cd "$INSTALL_DIR"

echo "备份当前数据..."
backup_output="$("$INSTALL_DIR/scripts/backup.sh")"
echo "$backup_output"

echo "拉取最新镜像..."
docker compose pull

echo "重启服务..."
docker compose up -d

echo "验证服务状态..."
for i in $(seq 1 30); do
    if curl -sf "http://localhost:${SERVER_PORT}/health" >/dev/null 2>&1; then
        echo -e "${GREEN}升级成功！${NC}"
        exit 0
    fi
    printf '  health check %s/30 pending...\n' "$i"
    sleep 2
done

echo -e "${RED}服务未正常启动。请检查日志: docker compose logs${NC}" >&2
echo "备份输出:"
echo "$backup_output"
exit 1
