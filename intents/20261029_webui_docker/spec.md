# 需求规格说明书（Spec）: web-ui SPA 前端 Docker 镜像

> 依据 [`intent.md`](./intent.md)（状态：accepted）编写。

## 1. 目标

交付一个 `web-ui` 的 Docker 镜像，把 SPA 静态产物托管在 nginx 上，并在本地以容器方式运行起来，浏览器访问容器端口即完成**真实登录**（连到实际运行的 `rts-center`，而非前端内置 mock）。

## 2. 交付物清单

| 路径 | 类型 | 职责 |
|---|---|---|
| `web-ui/Dockerfile` | 新增 | 多阶段构建：bun 构建 → nginx 托管 |
| `web-ui/.dockerignore` | 新增 | 收敛构建上下文 |
| `web-ui/docker/nginx.conf` | 新增 | nginx 主配置（SPA fallback、缓存、gzip、`/api` 反代） |
| `web-ui/docker/entrypoint.sh` | 新增 | 渲染上游地址并以非 root 身份启动 nginx |
| `web-ui/README.md` | 修改 | 追加「容器化」章节 |
| `web-ui/package.json` | 修改 | 追加 `docker:build` / `docker:run` script |
| `intents/20261029_webui_docker/verify.md` | 新增 | 验证记录 |

## 3. 镜像规格

### 3.1 构建

- **构建上下文**：`web-ui/` 目录。
- **builder 阶段**：`oven/bun:1.3.14-alpine`
  - `bun install --frozen-lockfile`（用仓库现有的 `bun.lock`，保证依赖可复现）
  - `bun run build`（= `tsc -b && vite build`，产物在 `web-ui/dist/`）
  - 构建期通过 `ARG` 注入 `VITE_API_MODE`（默认 `real`）与 `VITE_API_BASE_URL`（默认 `/api/v1`）
- **runtime 阶段**：`nginx:alpine`
  - 拷贝 `dist/` 到 `/usr/share/nginx/html`
  - 拷贝 `docker/nginx.conf`、`docker/entrypoint.sh`

### 3.2 构建期环境变量

| 变量 | 默认值 | 说明 |
|---|---|---|
| `VITE_API_MODE` | `real` | `real` 表示前端请求真实后端；`mock` 表示前端内置 mock |
| `VITE_API_BASE_URL` | `/api/v1` | 前端请求的接口基址，默认同源 |

> Vite 会把 `VITE_` 前缀的环境变量在**构建期**内联进产物，因此这两个值改变后必须重新构建镜像，运行时再改无效。

### 3.3 运行时

| 项 | 值 |
|---|---|
| 监听端口 | `8080`（容器内） |
| 宿主映射 | `-p 8080:8080`（可自行改） |
| 运行身份 | 非 root（`nginx` 用户，`uid 101`） |
| 环境变量 `API_UPSTREAM` | `/api` 反代的上游，默认 `http://host.docker.internal:8080` |
| 健康检查 | `GET /healthz` 返回 200（nginx 自定义 location，不落到 SPA） |

### 3.4 nginx 行为规格

| 需求 | 实现 | 验收方法 |
|---|---|---|
| SPA history fallback | `location / { try_files $uri $uri/ /index.html; }` | `curl -I /login` → 200 + `text/html` |
| 带 hash 资源长缓存 | `location /assets/ { expires 1y; add_header Cache-Control "public, max-age=31536000, immutable"; }` | `curl -I /assets/<hash>.js` → `immutable` |
| 入口页不缓存 | `location = /index.html { add_header Cache-Control "no-cache"; }` | `curl -I /index.html` → `no-cache` |
| gzip | `gzip on` + `gzip_types`（html/css/js/json/svg…） | `curl -H 'Accept-Encoding: gzip' -I /assets/x.js` → `Content-Encoding: gzip` |
| `/api` 反代 | `location /api/ { proxy_pass ${API_UPSTREAM}; }` | `curl /api/v1/auth/captcha` → 后端 JSON |
| 反代头透传 | `proxy_set_header Host/ X-Real-IP / X-Forwarded-For / X-Forwarded-Proto` | 后端可见真实客户端 IP |
| 反代超时 | `proxy_connect_timeout 5s; proxy_read_timeout 60s;` | 慢接口不无限挂起 |
| 后端不可达 | nginx 返回 502；前端按 HTTP 错误处理，落到 `SERVER_ERROR` 文案「服务器开小差了，请稍后再试」 | 停掉后端后登录页出现错误提示且不白屏 |

## 4. 环境依赖

- Docker（含 Docker Desktop 的 WSL2 后端）：宿主 `127.0.0.1:8080` 可访问。
- 后端 `rts-center`：需在**宿主**上运行并监听 `0.0.0.0:8080`（默认只听 `127.0.0.1`，容器访问不到），并注入：
  - `RTS_CENTER_ADDR=0.0.0.0:8080`
  - `RTS_AUTH_USERS='admin:<口令>:系统管理员:super_admin'`（**口令只存在于运行环境，不入库**）
  - `RTS_JWT_SECRET`（建议 `openssl rand -hex 32`）
- 容器访问宿主：`host.docker.internal`（Docker Desktop 内置 DNS）。若在 Linux 原生 Docker 上，需 `--add-host=host.docker.internal:host-gateway` 或改用后端容器名。

## 5. 安全约束

- 仓库内**不出现**任何真实口令、密钥。README 与 Dockerfile 中出现的均为占位符（如 `<口令>`）。
- 镜像内不含 `.env.local`（本地开发配置，被 `.dockerignore` 排除）。
- 容器以非 root 运行。
- 默认不暴露后端端口，仅暴露前端 `8080`。

## 6. 与现有约束的冲突

| 现有约束 | 冲突点 | 处理 |
|---|---|---|
| `README.md` 记录 `VITE_API_MODE=mock` 为默认 | 本镜像默认打 `real` 包 | 镜像默认值由 `ARG` 显式给出，与开发期默认值解耦；README 补充说明两者差异 |
| `vite dev server` 的 `/api` 代理仅开发期生效 | 生产需要同源反代 | 由 nginx 承担，规则见 §3.4 |
| `rts-center` 默认只监听回环地址 | 容器无法访问宿主回环地址 | 部署时用 `RTS_CENTER_ADDR=0.0.0.0:8080` 放开；不改后端源码 |
| 本机 Docker 守护进程访问不通 Docker Hub | 无法 `FROM node:...` 拉取基础镜像 | 已在宿主侧拉取基础镜像并 `docker load` 进守护进程；`Dockerfile` 本身仍是标准写法，在有网环境可直接 `docker build` |

## 7. 验收标准

1. `docker build` 成功产出 `rts-web-ui:latest`。
2. 容器启动后 `curl http://127.0.0.1:8080/` 返回登录页 HTML（`200`）。
3. `curl -I http://127.0.0.1:8080/login` 返回 `200`（SPA fallback 生效）。
4. `curl http://127.0.0.1:8080/api/v1/auth/captcha` 返回后端 JSON（反代生效）。
5. 浏览器打开 `http://127.0.0.1:8080/` 显示登录页（**非** mock 演示账号提示），输入账号口令 + 图形验证码可登录成功并进入首页。
6. 停掉后端后登录，登录页出现错误提示（502 → `SERVER_ERROR`），不出现白屏或未捕获异常。
7. 镜像内不含 `.env.local`、不含任何口令。
