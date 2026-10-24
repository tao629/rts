# web-ui

RTS 搬运工 Web 控制台前端。承载平台的任务调度与链路监控界面。

当前版本交付 **登录页 + 路由守卫 + 占位首页**，认证接口以 mock 实现，接口契约见
`sdlc/20261024_登录页SPA设计/spec.md`。

## 技术栈

| 关注点 | 选型 |
|-------|-----|
| 框架 | Vite 8 + React 19 + TypeScript |
| 样式 | Tailwind CSS v4（CSS-first，`@theme` 定义设计令牌） |
| 组件 | shadcn/ui 风格基础件（Radix 原语 + cva 变体） |
| 路由 | react-router-dom 7 |
| 表单 | react-hook-form + zod 4 |
| 测试 | bun test（内置，无额外依赖） |

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

## 切换真实后端

复制 `.env.example` 为 `.env.local` 并修改：

```ini
VITE_API_MODE=real
VITE_API_BASE_URL=/api/v1
# 开发期把 /api 代理到后端
VITE_API_TARGET=http://127.0.0.1:8080
```

`VITE_API_MODE=mock`（默认）时走前端内置实现，`real` 时走 `fetch` 请求后端，
页面代码不感知差异。接口契约：

```
POST /api/v1/auth/login
  { "username": "admin", "password": "Admin@123", "remember": true }
  → 200 { "token", "tokenType": "Bearer", "expiresAt", "user": { id, username, displayName, roles } }
  → 4xx { "code": "INVALID_CREDENTIALS" | "ACCOUNT_DISABLED" | "ACCOUNT_LOCKED" | "RATE_LIMITED" | "VALIDATION_ERROR", "message": "..." }

POST /api/v1/auth/logout      # Authorization: Bearer <token> → 204
```

## 目录结构

```
src/
├─ api/auth/         认证接口契约、mock 实现、http 实现、模式切换
├─ auth/             登录态存取、AuthProvider、路由守卫
├─ components/
│  ├─ ui/            shadcn 风格基础件（button/input/label/checkbox/alert）
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
