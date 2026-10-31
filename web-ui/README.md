# web-ui

RTS 搬运工 Web 控制台前端。承载平台的任务调度与链路监控界面。

当前版本交付 **登录页 + 路由守卫 + 占位首页**。认证走**真实后端 `rts-center`**
（图形验证码 + 登录 + 登出 + 确认登录态），前端不再内置任何模拟实现；
接口契约见 `intents/20261031_ui-login-real-backend/spec.md`。

## 技术栈

| 关注点 | 选型 |
|-------|-----|
| 框架 | Vite 8 + React 19 + TypeScript |
| 样式 | Tailwind CSS v4（CSS-first，`@theme` 定义设计令牌） |
| 组件 | shadcn/ui 风格基础件（Radix 原语 + cva 变体） |
| 路由 | react-router-dom 7 |
| 表单 | react-hook-form + zod 4 |
| 测试 | bun test（内置，无额外依赖） |
| 部署 | Docker 多阶段构建（bun 构建 → nginx 托管） |

## 快速开始

```bash
bun install
bun run dev        # 开发服务器
bun run build      # 类型检查 + 生产构建
bun run lint       # oxlint
bun run test       # 单元测试
```

## 联调真实后端

前端**始终**请求真实后端，没有模式开关。先起后端（账号在数据库里，
建表与开户见 `server/rts-center/README.md`），再起前端：

```bash
# 仓库根目录：起后端
export DATABASE_URL='postgresql://用户:口令@主机:5432/库名'
export RTS_JWT_SECRET="$(openssl rand -hex 32)"
cargo run -p rts-center

# web-ui/：起前端（/api 自动代理到后端）
bun run dev
```

前端始终**同源**请求 `/api/v1`：开发期由 vite dev server 代理，部署期由 nginx 反代，
因此前端不需要处理 CORS。需要换后端地址时复制 `.env.example` 为 `.env.local`：

```ini
VITE_API_BASE_URL=/api/v1
# 仅开发期：vite 把 /api 代理到后端，默认 http://127.0.0.1:8080
VITE_API_TARGET=http://127.0.0.1:8080
```

账号由后端从 `auth_user` 表读取，前端不保存任何真实口令。
接口契约：

```
GET /api/v1/auth/captcha
  → 200 { "captchaId": "...", "image": "data:image/svg+xml;base64,...", "expiresAt": "2026-10-25T02:15:30Z" }
  → 4xx/5xx { "code": "SERVER_ERROR" | ..., "message": "..." }

POST /api/v1/auth/login
  { "username": "admin", "password": "<口令>", "remember": true, "captchaId": "...", "captchaCode": "8F3K" }
  → 200 { "token", "tokenType": "Bearer", "expiresAt", "user": { id, username, displayName, roles } }
  → 4xx { "code": "INVALID_CAPTCHA" | "INVALID_CREDENTIALS" | "ACCOUNT_DISABLED" | "ACCOUNT_LOCKED" | "RATE_LIMITED" | "VALIDATION_ERROR", "message": "..." }

POST /api/v1/auth/logout      # Authorization: Bearer <token> → 204

GET /api/v1/auth/me           # Authorization: Bearer <token> → 200 { id, username, displayName, roles }
```

### 登录态由后端裁决

刷新页面时，前端拿本地存的令牌调 `GET /auth/me` 换一次用户信息：

- 200 → 用后端返回的用户信息刷新本地会话；
- 401 `TOKEN_INVALID` / 403 `ACCOUNT_DISABLED` → 清理本地凭证，按未登录处理；
- 网络错误 → **保留**本地会话（网络抖动不该把人踢出登录态）。

### 验证码说明

- 图形验证码由服务端渲染为 SVG，以 `data:image/svg+xml;base64,...` 随 JSON 返回，
  响应头带 `Cache-Control: no-store`；答案不在响应体中出现。
- 字符集 `23456789ABCDEFGHJKMNPQRSTUVWXYZ`（剔除易混字符），长度 4，大小写不敏感，
  有效期 300 秒。
- **一次性**：任何一次校验尝试（成功或失败）后立即失效，重复使用旧码一律判失败；
  服务端**先校验验证码、后核对账号口令**，因此账号口令错误时验证码同样作废。
- 错误码 `INVALID_CAPTCHA` → 前端提示「验证码错误或已失效，请重新输入」。
- 前端行为：页面挂载自动取码，点击图片刷新（支持 Enter/Space 键），任何一次登录失败后
  自动刷新验证码并清空输入框。

## 目录结构

```
src/
├─ api/auth/         认证接口契约与 http 实现
├─ auth/             登录态存取、AuthProvider、路由守卫
├─ components/
│  ├─ ui/            shadcn 风格基础件（button/input/label/checkbox/alert）
│  ├─ captcha-field.tsx 验证码输入框 + 可刷新的验证码图片
│  └─ brand-panel.tsx 登录页左侧深色品牌区
├─ lib/              cn、登录校验 schema、跳转目标消毒
├─ pages/            login-page / dashboard-page
└─ routes/           路由表
```

## 设计令牌

令牌集中在 `src/index.css` 的 `@theme` 中：品牌色 `--color-brand-*`、深色面板
`--color-panel-*`、文字描边 `--color-ink-*`、圆角 `--radius-field/card`。
新增页面直接使用 `bg-brand-500`、`text-ink-500` 等工具类即可，无需另起一套颜色。

## 登录态

- 勾选「记住我」→ 凭证写入 `localStorage`；未勾选 → 写入 `sessionStorage`。
- 读取时校验 `expiresAt`，过期自动清理并视为未登录。
- 未登录访问受保护页面会重定向到 `/login?from=<原路径>`，登录成功后跳回。
- 页面挂载时向后端 `GET /auth/me` 确认登录态：令牌失效就清凭证回登录页；
  网络故障则保留本地登录态（详见上文「登录态由后端裁决」）。

## 容器化

前端打包为 Docker 镜像：`bun` 构建产物 → `nginx` 托管，提供 SPA 路由回退、
静态资源缓存与 `/api` 反向代理。

### 构建

```bash
cd web-ui
docker build -t rts-web-ui:latest .
# 或
bun run docker:build
```

镜像默认请求**真实后端**。构建参数（Vite 在构建期把 `VITE_` 前缀变量内联进产物，
**改完必须重新构建**）：

| 构建参数 | 默认值 | 说明 |
|---|---|---|
| `--build-arg VITE_API_BASE_URL=/api/v1` | `/api/v1` | 前端请求的接口基址 |

### 运行

后端 `rts-center` 需要先在**宿主**上启动，并且监听地址要放开到回环地址之外
（默认只听 `127.0.0.1`，容器访问不到）：

```bash
export RTS_CENTER_ADDR=0.0.0.0:8080
export DATABASE_URL='postgresql://用户:口令@主机:5432/库名'
export RTS_JWT_SECRET="$(openssl rand -hex 32)"
cargo run -p rts-center
```

> 口令与密钥只从环境变量注入，**不要写进仓库**。

再启动前端容器：

```bash
docker run -d --name rts-web-ui -p 18080:8080 \
  -e API_UPSTREAM=http://host.docker.internal:8080 \
  rts-web-ui:latest
```

浏览器访问 <http://127.0.0.1:18080/>。

> **宿主端口别跟后端撞车**：后端已经占用 `8080` 时，`-p 8080:8080` 的端口转发会**静默失效**
> （容器能起、`docker port` 却查不到映射），所以上面用 `18080`。
> 换端口后记得同步改浏览器访问地址。

| 运行时环境变量 | 默认值 | 说明 |
|---|---|---|
| `API_UPSTREAM` | `http://host.docker.internal:8080` | `/api` 反代的上游地址 |

后端与前端同在容器网络里时，把上游改成后端服务名即可，例如
`-e API_UPSTREAM=http://rts-center:8080`；Linux 原生 Docker 上访问宿主需加
`--add-host=host.docker.internal:host-gateway`。

### 容器行为

| 路径 | 行为 |
|---|---|
| `/`、`/login` 等前端路由 | SPA history fallback，返回 `index.html` |
| `/assets/*` | 带内容 hash，`Cache-Control: public, max-age=31536000, immutable` |
| `/index.html` | `Cache-Control: no-cache`，保证发版后拿到新资源 |
| `/api/*` | 反代到 `API_UPSTREAM`，透传 `Host` / `X-Real-IP` / `X-Forwarded-*` |
| `/healthz` | 健康检查，容器 `HEALTHCHECK` 亦使用该端点 |

容器以**非 root**（`nginx` 用户）身份运行，容器内监听 **8080**，宿主端口由
`-p` 决定。

### 目录结构补充

```
web-ui/
├─ Dockerfile          # 多阶段构建
├─ .dockerignore       # 构建上下文收敛
└─ docker/
   ├─ nginx.conf       # SPA 回退 / 缓存 / gzip / /api 反代
   └─ entrypoint.sh    # 渲染上游地址 → nginx -t → 前台启动
```
