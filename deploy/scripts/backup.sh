#!/bin/bash
set -euo pipefail

INSTALL_DIR="${SYMPHONY_INSTALL_DIR:-$HOME/.symphony}"
BACKUP_ROOT="${SYMPHONY_BACKUP_DIR:-$INSTALL_DIR/backups}"
BACKUP_DIR="$BACKUP_ROOT/$(date +%Y%m%d_%H%M%S)"

if [ ! -f "$INSTALL_DIR/docker-compose.yml" ]; then
    echo "未找到 $INSTALL_DIR/docker-compose.yml，无法备份" >&2
    exit 1
fi

mkdir -p "$BACKUP_DIR"
cd "$INSTALL_DIR"

if [ -f "$INSTALL_DIR/.env" ]; then
    cp "$INSTALL_DIR/.env" "$BACKUP_DIR/.env"
    chmod 600 "$BACKUP_DIR/.env"
fi

if docker compose ps --status running --services | grep -qx symphony; then
    if docker compose exec -T symphony sh -c 'test -f /data/symphony.db && cat /data/symphony.db' > "$BACKUP_DIR/symphony.db"; then
        echo "数据库备份: $BACKUP_DIR/symphony.db"
    else
        rm -f "$BACKUP_DIR/symphony.db"
        echo "警告: 未能从运行中容器备份 /data/symphony.db" >&2
    fi

    if docker compose exec -T symphony sh -c 'tar -C /workspaces -czf - .' > "$BACKUP_DIR/workspaces.tar.gz"; then
        echo "工作空间备份: $BACKUP_DIR/workspaces.tar.gz"
    else
        rm -f "$BACKUP_DIR/workspaces.tar.gz"
        echo "警告: 未能备份 /workspaces" >&2
    fi
else
    echo "警告: symphony 容器未运行，仅备份本地 .env" >&2
fi

echo "备份目录: $BACKUP_DIR"
