# web-ui

RTS 搬运工 Web 控制台前端。承载平台的任务调度与链路监控界面。

当前版本交付 **登录页 + 路由守卫 + 占位首页**，认证接口以 mock 实现，接口契约见
`sdlc/20261024_登录页SPA设计/spec.md`；登录验证码的扩展契约见
`sdlc/20261025_登录验证码/spec.md`。

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

## 演示账号（mock 模式）

| 用户名 | 密码 | 角色 |
|-------|-----|-----|
| `admin` | `Admin@123` | super_admin |
| `operator` | `Operator@123` | operator |

> mock 模式同样需要填写图形验证码，请照着登录页上的图片输入。

## 切换真实后端

复制 `.env.example` 为 `.env.local` 并修改：

```ini
VITE_API_MODE=real
VITE_API_BASE_URL=/api/v1
# 开发期把 /api 代理到后端
VITE_API_TARGET=http://127.0.0.1:8080
```

`VITE_API_MODE=mock`（默认）时走前端内置实现，`real` 时走 `fetch` 请求后端，
页面代码不感知差异。real 模式下的账号由后端通过 `RTS_AUTH_USERS` 环境变量注入，
前端不保存任何真实口令，配置方式见 `server/rts-center/README.md`。接口契约：

```
GET /api/v1/auth/captcha
  → 200 { "captchaId": "...", "image": "data:image/svg+xml;base64,...", "expiresAt": "2026-10-25T02:15:30Z" }
  → 4xx/5xx { "code": "SERVER_ERROR" | ..., "message": "..." }

POST /api/v1/auth/login
  { "username": "admin", "password": "<口令>", "remember": true, "captchaId": "...", "captchaCode": "8F3K" }
  → 200 { "token", "tokenType": "Bearer", "expiresAt", "user": { id, username, displayName, roles } }
  → 4xx { "code": "INVALID_CAPTCHA" | "INVALID_CREDENTIALS" | "ACCOUNT_DISABLED" | "ACCOUNT_LOCKED" | "RATE_LIMITED" | "VALIDATION_ERROR", "message": "..." }

POST /api/v1/auth/logout      # Authorization: Bearer <token> → 204
```

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

### mock 模式下的验证码

`VITE_API_MODE=mock` 时没有后端，前端会在 `MockAuthApi` 内部按同一规格生成验证码
（同字符集、4 位、260×60 SVG、带噪点与干扰线），答案只保存在 mock 实现内部的内存表里，
前端页面读不到。因此 mock 模式下也需要照着图片输入验证码才能登录，
这与 real 模式的交互完全一致。

## 目录结构

```
src/
├─ api/auth/         认证接口契约、mock 实现、http 实现、模式切换
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

镜像默认按 **real 模式**打包（前端请求真实后端）。构建参数（Vite 在构建期把
`VITE_` 前缀变量内联进产物，**改完必须重新构建**）：

| 构建参数 | 默认值 | 说明 |
|---|---|---|
| `--build-arg VITE_API_MODE=real\|mock` | `real` | `real` 请求真实后端；`mock` 用前端内置 mock |
| `--build-arg VITE_API_BASE_URL=/api/v1` | `/api/v1` | 前端请求的接口基址 |

例：打一个不依赖后端的 mock 包 → `docker build --build-arg VITE_API_MODE=mock -t rts-web-ui:mock .`

### 运行

后端 `rts-center` 需要先在**宿主**上启动，并且监听地址要放开到回环地址之外
（默认只听 `127.0.0.1`，容器访问不到）：

```bash
export RTS_CENTER_ADDR=0.0.0.0:8080
export RTS_AUTH_USERS='admin:<口令>:系统管理员:super_admin'
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
