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
（同字符集、4 位、160×60 SVG、带噪点与干扰线），答案只保存在 mock 实现内部的内存表里，
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
