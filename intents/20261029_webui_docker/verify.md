# 验证记录（Verify）: web-ui SPA 前端 Docker 镜像

> 依据 [`spec.md`](./spec.md) §7 验收标准逐条核对。验证日期：2026-10-29。
> 开发分支：`feature/webui-docker`（worktree: `.claude/worktrees/webui-docker/`）

## 0. 验证环境

| 项 | 值 |
|---|---|
| 镜像 | `rts-web-ui:latest`（多阶段：bun 构建 → nginx 托管） |
| 容器 | `rts-web-ui`，宿主 `18080` → 容器 `8080` |
| 后端 | `rts-center` 跑在宿主 `0.0.0.0:8080`（`RTS_CENTER_ADDR` 放开） |
| 上游 | `API_UPSTREAM=http://host.docker.internal:8080` |
| Docker | 29.8.2（Docker Desktop 4.94.0，WSL2 后端） |
| 浏览器 | Windows Chrome 156 Headless + CDP（`--remote-debugging-port=9222`） |

> 账号口令与 JWT 密钥均由环境变量注入，**未写入仓库、未写入镜像**。

## 1. 构建

```
docker build -t rts-web-ui:latest .
```

| 阶段 | 结果 |
|---|---|
| builder `bun install --frozen-lockfile` | 通过（按 `bun.lock` 锁版本） |
| builder `bun run build`（`tsc -b && vite build`） | 通过，2080 modules transformed |
| 产物 | `dist/index.html` 0.64 kB、`dist/assets/index-*.css` 30.28 kB、`dist/assets/index-*.js` 451.09 kB |
| runtime 镜像导出 | 成功 |

> **构建环境注意**：本机 Docker 守护进程所在 VM 访问不通 Docker Hub（`registry-1.docker.io` 直连超时），
> 基础镜像 `nginx:alpine`、`oven/bun:1.3.14-alpine` 是在宿主机侧经代理拉取后 `docker load` 进守护进程的。
> 构建阶段访问 npm registry 正常，**Dockerfile 本身是标准写法，在有网环境可直接 `docker build`**。

## 2. HTTP 冒烟（对照 spec §7）

| # | 验收项 | 实测 | 结论 |
|---|---|---|---|
| 1 | `docker build` 产出 `rts-web-ui:latest` | 成功 | ✅ |
| 2 | `GET /` 返回登录页 HTML | `200`，`text/html`，641 B | ✅ |
| 3 | `GET /login` SPA fallback | `200`，内容与 `/` 完全一致 | ✅ |
| 4 | `GET /healthz` | `200`，body `ok` | ✅ |
| 5 | `/assets/<hash>.js` 缓存头 | `Cache-Control: public, max-age=31536000, immutable` | ✅ |
| 6 | gzip | `Content-Encoding: gzip` | ✅ |
| 7 | `/index.html` 缓存头 | `Cache-Control: no-cache` | ✅ |
| 8 | `/api/v1/auth/captcha` 反代 | `200`，返回后端 JSON（`captchaId` + base64 SVG） | ✅ |
| 9 | 容器身份 | `uid=101(nginx) gid=101(nginx)` —— 非 root | ✅ |
| 10 | 容器 `HEALTHCHECK` | `healthy` | ✅ |

补充检查：

- 镜像内 `/usr/share/nginx/html` **无任何 `.env*` 文件**（`.env.local` 已被 `.dockerignore` 排除）。
- 全量 grep `LocalTest` / `RTS_JWT_SECRET` / 密钥串 → **无命中**，镜像未泄露凭据。
- `nginx -t` 在 entrypoint 中执行并通过：`syntax is ok` / `test is successful`。

## 3. 真实登录链路

### 3.1 接口层

```
GET  /api/v1/auth/captcha → 200
POST /api/v1/auth/login    → 200
  tokenType = Bearer
  user      = {"id":"u-1001","username":"admin","displayName":"系统管理员","roles":["super_admin"]}
```

### 3.2 浏览器层（Windows Chrome Headless + CDP 驱动）

| 步骤 | 实测 |
|---|---|
| 打开 `http://localhost:18080/login` | 登录页正常渲染 |
| 页面出现 mock 演示账号提示 | **false** —— 确认镜像按 real 模式打包 |
| 验证码图片 | 来自真实后端（`img[alt="验证码"]` 的 src 为后端返回的 base64 SVG） |
| 填表提交 | 用户名 / 密码 / 验证码，React 受控输入用原生 setter + `input` 事件写入 |
| 登录后 URL | `http://localhost:18080/` |
| 首页断言 | 含「你好，系统管理员」✅、含「链路健康度」✅、含「退出」按钮 ✅ |
| 提示 | 顶部出现「登录成功」toast |

截图：`/tmp/rts_webui_01_login.png`、`/tmp/rts_webui_02_dashboard.png`。

### 3.3 后端不可用时的表现

停掉 `rts-center` 后重新加载登录页并提交：

| 观察 | 实测 |
|---|---|
| 错误提示 | 「服务器开小差了，请稍后再试」 |
| 是否白屏 | 否，`#root` 渲染内容长度 12642 |
| 验证码区 | 显示加载中（取码接口 502） |

**与初稿 spec 的偏差（已修订）**：初稿预期后端不可达时提示 `NETWORK_ERROR`
（「无法连接到服务器」）。实测 nginx 对 `/api` 返回 **HTTP 502**，`fetch` 本身成功、
失败在 `!response.ok` 分支，故落到 `SERVER_ERROR` 文案。行为本身正确（有明确提示、
不白屏），spec §3.4 / §7.6 已按实测修订。

## 4. 已知限制

1. **宿主端口不能与后端同端口**：`rts-center` 已占用 WSL 的 `8080`，此时 `-p 8080:8080`
   的端口转发会静默失效（`NetworkSettings.Ports` 为空、`docker port` 无输出）。
   本次改用 `-p 18080:8080`。宿主端口需自行避开后端端口。
2. **Docker Hub 直连不通**：见 §1 备注，基础镜像需在有网环境预拉取或改配代理。
3. **`VITE_*` 构建期内联**：切换 mock / real 必须重新构建镜像，运行时改环境变量无效。
4. **后端需放开监听地址**：`rts-center` 默认只听 `127.0.0.1`，容器访问不到，
   部署时须设 `RTS_CENTER_ADDR=0.0.0.0:8080`。
5. **验证码可被解码**：后端把字符以 `<text>` 元素画进 SVG，前端拿到 base64 后可直接解出答案
   （本次验证即利用了这一点）。这是**后端既有设计**的问题，不在本次容器化范围内，
   但意味着该图形验证码对自动化脚本基本无拦截力，建议后续单独立项处理。

## 5. 结论

spec §7 的 7 条验收标准**全部通过**（其中第 6 条按 §3.3 修订后通过）。
镜像可交付，容器已在本地跑通完整登录链路。
