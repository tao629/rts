#!/bin/sh
# RTS web-ui 容器启动脚本
#
# 职责：渲染 nginx 配置里的上游地址 → 校验配置 → 前台启动 nginx。
# 以非 root（nginx 用户）身份运行。

set -eu

: "${API_UPSTREAM:=http://host.docker.internal:8080}"
export API_UPSTREAM

CONF_TEMPLATE=/etc/nginx/nginx.conf.template
CONF_OUTPUT=/etc/nginx/nginx.conf

# 转义 sed 替换串里的 & | \ ，避免上游地址里的特殊字符被当成替换语义
escaped=$(printf '%s' "$API_UPSTREAM" | sed 's/[&|\\]/\\&/g')

sed "s|__API_UPSTREAM__|${escaped}|g" "$CONF_TEMPLATE" > "$CONF_OUTPUT"

echo "[entrypoint] API 上游: ${API_UPSTREAM}"
nginx -t

exec nginx -g 'daemon off;'
