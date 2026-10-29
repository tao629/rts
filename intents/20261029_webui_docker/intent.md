# 意图（Intent）: web-ui SPA 前端容器化，产出 Docker 镜像并本地部署运行

Author: zhtao. 状态：accepted

## 问题（Problem）

`web-ui` 是 Vite 8 + React 19 的纯前端 SPA，目前只能靠 `bun run dev` / `bun run preview` 在本机跑，没有可交付的容器化产物。具体痛点：

1. **交付环境不可复现**：部署到内网服务器或离线机器时，需要在目标机上装 bun/node 并现场构建，依赖拉取受网络限制，步骤随人变化，出错难定位。
2. **缺少统一的静态资源运行载体**：SPA 的 history fallback（`/login` 等前端路由直接刷新）、静态资源长缓存、`index.html` 不缓存、gzip 等配置目前完全空白，每换一个部署环境就要重配一遍。
3. **同源访问 `/api` 没有生产态方案**：`vite dev server` 的 proxy 只在开发期生效。后端 `rts-center` 默认只监听 `127.0.0.1:8080`，前端与后端分离部署时，浏览器同源请求 `/api/v1` 需要一层反向代理，目前只有开发期这一种实现。

## 预期结果（Proposed outcome）

- 仓库内提供 `web-ui` 的 Dockerfile，一条 `docker build` 产出可直接 `docker run` 的镜像（tag 如 `rts-web-ui:latest`）。
- 镜像基于 nginx 托管 `vite build` 的产物（`dist/`），满足以下可度量行为：
  - **SPA history fallback**：访问 `/login`、`/login?from=/` 等前端路由返回 200 + `index.html`，而不是 404。
  - **缓存策略**：`/assets/` 下带 hash 的产物 `Cache-Control: public, max-age=31536000, immutable`；`index.html` 为 `no-cache`。
  - **压缩**：文本类资源（html/js/css/svg/json）走 gzip。
  - **反代**：`/api` 前缀的请求转发到 `rts-center`，上游地址可用环境变量切换。
- 镜像按 **real 模式**打包（`VITE_API_MODE=real`），前端请求真实后端 `/api/v1`，而非前端内置 mock。
- 一条 `docker run` 命令即可在本地把容器跑起来：浏览器访问容器端口即见登录页，并能完成**真实登录**（连到实际运行的 `rts-center`），而非 mock 登录。
- 交付验证记录：镜像构建结果、容器启动日志、HTTP 冒烟结果、登录链路（验证码 + 登录）验证结论。

## 受影响的模块（Affected modules）

| 文件 / 目录 | 改动 | 说明 |
|---|---|---|
| `web-ui/Dockerfile` | 新增 | 多阶段构建：bun 构建 → nginx 托管 |
| `web-ui/.dockerignore` | 新增 | 排除 `node_modules`、`dist`、`.env.local` 等 |
| `web-ui/docker/nginx.conf` | 新增 | SPA fallback、缓存、gzip、`/api` 反代 |
| `web-ui/docker/entrypoint.sh` | 新增（可选） | 渲染上游地址、非 root 启动 nginx |
| `web-ui/README.md` | 修改 | 补充容器化构建与运行说明 |
| `web-ui/package.json` | 修改（可选） | 增加 `docker:build` / `docker:run` 等便捷 script |
| `intents/20261029_webui_docker/` | 新增 | 立项 / 设计 / 计划 / 验证记录 |

**明确不改动**：`server/rts-center` 源码。本次只交付前端镜像，后端在宿主上以普通进程运行，仅需把 `RTS_CENTER_ADDR` 放开到 `0.0.0.0:8080` 才能被容器访问。

## 约束条件（Constraints）

- **凭据不入库**：`RTS_AUTH_USERS`、`RTS_JWT_SECRET` 等一律由运行环境注入，仓库、Dockerfile、README 示例中均不出现真实口令或密钥。
- **构建可复现**：使用仓库现有的 `bun.lock` 锁依赖，builder 阶段执行 `bun install --frozen-lockfile`。
- **构建上下文收敛**：以 `web-ui/` 为上下文，`.dockerignore` 排除 `node_modules`、`dist`、`.env.local`（本地开发文件，且被 gitignore）。
- **构建参数显式化**：镜像内的 `VITE_API_MODE` / `VITE_API_BASE_URL` 由 Dockerfile 的 `ARG`/`ENV` 给出，不依赖镜像外的 `.env.local`。
- **本次不交付 compose**：只产出 `web-ui` 镜像，后端与编排方式由部署方自理。
- **非 root 运行**：nginx 官方镜像默认以 root 启动并占用 80，改为监听 **8080** 并以非 root 身份运行（需调整配置目录属主与 pid 路径）。
- **不追求极限瘦身**：基础镜像用 `nginx:alpine`，本期不做 distroless 之类的激进裁剪。

## 未解决的问题（Open questions）

已全部拍板：

| # | 问题 | 决议 |
|---|---|---|
| 1 | `/api` 上游默认地址取什么？ | 默认 `http://host.docker.internal:8080`（Docker Desktop 下访问宿主机的 `rts-center`），支持环境变量 `API_UPSTREAM` 覆盖为 `http://rts-center:8080`（compose / k8s 场景） |
| 2 | 镜像内 nginx 监听端口 | **8080**（非 root 身份不能绑定 80，除非额外放开 `CAP_NET_BIND_SERVICE`） |
| 3 | real 模式下前端登录失败时是否需要区分「后端未启动」 | 由 nginx 返回 502，前端按 HTTP 错误呈现「服务器开小差了，请稍后再试」，不额外做前端改造 |
