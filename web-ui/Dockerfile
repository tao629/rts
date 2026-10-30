# RTS web-ui 镜像
#
# 多阶段构建：
#   builder —— 用 bun 按 bun.lock 安装依赖并产出 vite 构建结果（dist/）
#   runtime —— 用 nginx 托管 dist/，负责 SPA history fallback、静态资源缓存与 /api 反代
#
# 构建：
#   docker build -t rts-web-ui:latest web-ui
#
# 运行（real 模式，前端请求真实后端）：
#   docker run -d --name rts-web-ui -p 8080:8080 \
#     -e API_UPSTREAM=http://host.docker.internal:8080 rts-web-ui:latest

# ---------- 构建阶段 ----------
FROM oven/bun:1.3.14-alpine AS builder

WORKDIR /app

# Vite 在构建期把 VITE_ 前缀的变量内联进产物：改这两个值必须重新构建镜像
ARG VITE_API_MODE=real
ARG VITE_API_BASE_URL=/api/v1
ENV VITE_API_MODE=${VITE_API_MODE} \
    VITE_API_BASE_URL=${VITE_API_BASE_URL}

# 依赖层单独拆开：只有 lockfile 变化才会让这一层失效
COPY package.json bun.lock ./
RUN bun install --frozen-lockfile

COPY . .

# tsc -b && vite build → /app/dist
RUN bun run build

# ---------- 运行阶段 ----------
FROM nginx:alpine

# /api 的上游地址，运行时可覆盖（compose / k8s 场景改为后端服务名）
ENV API_UPSTREAM=http://host.docker.internal:8080

COPY --from=builder /app/dist /usr/share/nginx/html
COPY docker/nginx.conf /etc/nginx/nginx.conf.template
COPY docker/entrypoint.sh /usr/local/bin/entrypoint.sh

# 非 root 运行：修正属主并把 pid/临时目录放到可写位置
RUN chmod +x /usr/local/bin/entrypoint.sh \
    && chown -R nginx:nginx /usr/share/nginx/html /etc/nginx \
    && mkdir -p /tmp/nginx \
    && chown -R nginx:nginx /tmp/nginx

USER nginx

EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD wget -q -O- http://127.0.0.1:8080/healthz || exit 1

ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
