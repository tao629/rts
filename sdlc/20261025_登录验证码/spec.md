# 需求规格说明书（Spec）: 登录验证码（服务端生成图形验证码）

关联文档: [intent.md](./intent.md)（状态：Final）
适用版本: v1.0 | 日期：2026-10-25

> 本文档定义「做什么」，不涉及实现细节。前后端并行开发，**本文第 3、4 节是前后端的唯一契约来源**，两侧不得各自改动字段名。

---

## 1. 总体流程

```
浏览器                          rts-center
  │  GET /api/v1/auth/captcha
  ├──────────────────────────────▶  生成验证码（ID + 答案 + 图片），答案入内存存储（TTL 5min）
  │  ◀──────────────────────────────  { captchaId, image, expiresAt }
  │
  │  POST /api/v1/auth/login
  │  { username, password, remember, captchaId, captchaCode }
  ├──────────────────────────────▶  ① 校验验证码（一次性消费）──失败──▶ 400 INVALID_CAPTCHA
  │                                   ② 校验账号口令────────失败──▶ 401 INVALID_CREDENTIALS
  │                                   ③ 校验失败锁定──────超阈值──▶ 423 ACCOUNT_LOCKED
  │                                   ④ 签发令牌
  │  ◀──────────────────────────────  { token, tokenType, expiresAt, user }
```

**关键约束：验证码校验先于账号口令校验**。即便账号口令错误，验证码也已被消费，客户端必须重新取码。

---

## 2. 验证码规格

| 项 | 规格 |
|----|------|
| 形态 | 图形验证码，服务端渲染为 SVG，以 `data:image/svg+xml;base64,<base64>` 形式随 JSON 返回 |
| 尺寸 | 160 × 60 |
| 字符集 | `23456789ABCDEFGHJKMNPQRSTUVWXYZ`（剔除 `0/O/1/I/L` 等易混字符），长度 4 |
| 大小写 | 不敏感（比较前统一转大写） |
| 有效期 | 300 秒（5 分钟），可由服务端配置项覆盖 |
| 一次性 | 任何一次校验尝试（成功或失败）后立即失效；重复使用旧码一律判失败 |
| 答案回传 | **禁止**在响应中出现验证码明文答案 |
| 存储 | 服务端进程内存，按 `captchaId` 索引，惰性清理过期项，容量上限（默认 10000）超出时清理最旧项 |
| 服务端日志 | 不记录验证码答案、不记录用户口令 |

---

## 3. HTTP 接口契约（服务端 ↔ 前端）

服务基址：`/api/v1`；所有响应 `Content-Type: application/json`（登出为 204 空响应）。
**错误响应统一结构**：`{ "code": "<错误码>", "message": "<可读文案>" }`。

### 3.1 获取验证码

```
GET /api/v1/auth/captcha
```

响应 200：

```json
{
  "captchaId": "b3f1c0d2-8a41-4f0e-9c7b-1d2e3f4a5b6c",
  "image": "data:image/svg+xml;base64,PHN2ZyB4bWxucz0i...",
  "expiresAt": "2026-10-25T02:15:30Z"
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `captchaId` | string | 验证码唯一标识，登录时原样回传 |
| `image` | string | `data:image/svg+xml;base64,` 前缀的 SVG 图片，可直接用于 `<img src>` |
| `expiresAt` | string | ISO 8601 时间戳（UTC），验证码过期时刻 |

响应头：`Cache-Control: no-store`。

### 3.2 登录

```
POST /api/v1/auth/login
Content-Type: application/json
```

请求体：

```json
{
  "username": "admin",
  "password": "<口令>",
  "remember": true,
  "captchaId": "b3f1c0d2-8a41-4f0e-9c7b-1d2e3f4a5b6c",
  "captchaCode": "8F3K"
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `username` | string | 是 | 去首尾空格后非空，长度 ≤ 64 |
| `password` | string | 是 | 非空，长度 ≤ 128 |
| `remember` | bool | 否 | 缺省视为 `false` |
| `captchaId` | string | 是 | 缺失或为空即拒绝 |
| `captchaCode` | string | 是 | 缺失或为空即拒绝；大小写不敏感 |

响应 200（结构与既有前端契约 `LoginResponse` 一致）：

```json
{
  "token": "<JWT>",
  "tokenType": "Bearer",
  "expiresAt": "2026-10-26T02:14:30Z",
  "user": {
    "id": "u-1001",
    "username": "admin",
    "displayName": "系统管理员",
    "roles": ["super_admin"]
  }
}
```

令牌有效期：`remember=true` 为 24 小时，`false` 为 2 小时。

### 3.3 登出

```
POST /api/v1/auth/logout
Authorization: Bearer <token>
```

响应 204 无内容。客户端本地清理凭证不依赖该接口成功。

### 3.4 健康检查

```
GET /api/v1/health
```

响应 200：`{ "status": "ok", "time": "<ISO 8601>" }`。

### 3.5 错误码与 HTTP 状态映射

| 错误码 | HTTP | 触发条件 |
|--------|------|----------|
| `VALIDATION_ERROR` | 400 | 必填字段缺失、字段超长、请求体非法 |
| `INVALID_CAPTCHA` | 400 | 验证码缺失 / 不匹配 / 过期 / 已使用 |
| `INVALID_CREDENTIALS` | 401 | 用户名或口令错误 |
| `ACCOUNT_LOCKED` | 423 | 同一账号连续失败超阈值，锁定期内再次登录 |
| `RATE_LIMITED` | 429 | 触发限流（本次与账号锁定同源，保留该码以兼容前端既有枚举） |
| `SERVER_ERROR` | 500 | 服务端内部异常 |

> 前端既有 `AuthErrorCode` 枚举需**新增** `INVALID_CAPTCHA`，其余保持不变。

---

## 4. 前端契约扩展（`web-ui/src/api/auth`）

### 4.1 类型

```ts
export interface CaptchaResponse {
  captchaId: string
  /** data:image/svg+xml;base64,... */
  image: string
  /** ISO 8601 时间戳 */
  expiresAt: string
}

export interface LoginRequest {
  username: string
  password: string
  remember: boolean
  captchaId: string
  captchaCode: string
}

export type AuthErrorCode =
  | "VALIDATION_ERROR"
  | "INVALID_CAPTCHA"      // 新增
  | "INVALID_CREDENTIALS"
  | "ACCOUNT_DISABLED"
  | "ACCOUNT_LOCKED"
  | "RATE_LIMITED"
  | "NETWORK_ERROR"
  | "SERVER_ERROR"

export interface AuthApi {
  login(request: LoginRequest): Promise<LoginResponse>
  logout(token: string): Promise<void>
  fetchCaptcha(): Promise<CaptchaResponse>   // 新增
}
```

### 4.2 真实实现（`HttpAuthApi`）

- `fetchCaptcha()`：`GET {baseUrl}/auth/captcha`，`Cache-Control: no-store`，解析为 `CaptchaResponse`；网络异常抛 `NETWORK_ERROR`，非 2xx 抛 `SERVER_ERROR`。
- `login()`：请求体追加 `captchaId`、`captchaCode`；非 2xx 时沿用既有错误码读取逻辑（读取响应体 `code`）。

### 4.3 Mock 实现（`MockAuthApi`）

- `fetchCaptcha()`：在浏览器端生成同规格的 SVG 验证码（4 位同字符集字符），返回 `captchaId`（随机串）+ `data:image/svg+xml;base64,` 图片 + 过期时间；答案只留在 mock 实现内部。
- `login()`：先校验 `captchaId`/`captchaCode`（不匹配 / 缺失 / 已被使用）→ 抛 `INVALID_CAPTCHA`；再校验账号口令。
- mock 验证码同样一次性消费。

---

## 5. 登录页交互规格

| 元素 | 规格 |
|------|------|
| 验证码图片 | 位于验证码输入框右侧，宽 160px 高 60px；`alt="验证码"`；鼠标手型；**点击图片刷新验证码** |
| 验证码输入框 | `label` 文案「验证码」；`autocomplete="off"`；`inputMode="text"`；最大长度 8；提示「请输入图中字符」 |
| 加载态 | 验证码请求中显示占位/骨架，图片与输入框禁用 |
| 刷新时机 | ① 登录页挂载后首次自动获取；② 用户点击图片；③ **任何一次登录失败后自动刷新**并清空输入框 |
| 校验 | 必填，长度 4~6；错误文案「请输入验证码」「验证码格式不正确」 |
| 失败提示 | 错误码 `INVALID_CAPTCHA` → 「验证码错误或已失效，请重新输入」；其余沿用既有错误文案表 |
| 无障碍 | 图片为可聚焦按钮语义（`role="button"` + `tabIndex=0` + 键盘 Enter/Space 可刷新），`aria-label="点击刷新验证码"` |

---

## 6. 账号与环境变量配置

### 6.1 账号注入（`RTS_AUTH_USERS`）

账号**不内置在代码或文档里**，由环境变量注入；服务启动时对每个口令做一次加盐哈希，之后内存里只留哈希串。

格式：

```
RTS_AUTH_USERS=<用户名>:<口令>:<显示名>[:<角色1>,<角色2>];<用户名>:<口令>:<显示名>[:<角色1>,<角色2>]
```

| 规则 | 说明 |
|------|------|
| 条目分隔 | `;` |
| 字段分隔 | `:`，顺序为 `用户名` / `口令` / `显示名` / `角色列表` |
| 角色列表 | 可省略（该账号无角色）；多个角色用 `,` 分隔 |
| 口令字符 | 不得包含 `:` 与 `;` |
| 格式非法 | 启动时打印明确错误并退出，不静默跳过 |
| 未配置 | 服务正常启动，账号表为空，日志 warn「未配置账号，所有登录都会失败」；任何登录按 `INVALID_CREDENTIALS` 处理 |

### 6.2 配置项（环境变量）

| 变量 | 默认值 | 说明 |
|------|--------|------|
| `RTS_CENTER_ADDR` | `127.0.0.1:8080` | 监听地址 |
| `RTS_AUTH_USERS` | 空 | 账号清单，见 §6.1；为空则无可登录账号 |
| `RTS_JWT_SECRET` | 未配置时**启动时随机生成** | 令牌签名密钥；未配置时用随机密钥并 warn，令牌在进程重启后失效。**代码与文档中不得出现固定密钥串** |
| `RTS_CAPTCHA_TTL_SECS` | `300` | 验证码有效期（秒） |
| `RTS_LOGIN_MAX_FAILURES` | `5` | 触发锁定的连续失败次数 |
| `RTS_LOGIN_LOCK_SECS` | `60` | 锁定时长（秒） |

失败锁定：同一「用户名 + 来源 IP」连续失败达阈值后锁定；锁定期间登录返回 `ACCOUNT_LOCKED`；登录成功清零。

---

## 7. 验收标准

| # | 验收项 | 判定方式 |
|---|--------|----------|
| A1 | 服务端可生成验证码并返回图片 | `GET /api/v1/auth/captcha` 返回 200，body 含三项字段，`image` 以 `data:image/svg+xml;base64,` 开头 |
| A2 | 无验证码或错验证码无法登录 | 缺字段 / 错码 / 空串 → 400 `INVALID_CAPTCHA` |
| A3 | 验证码一次性 | 同一 `captchaId` 第一次校验成功后再次使用 → 400 `INVALID_CAPTCHA` |
| A4 | 验证码过期 | 等待 TTL 过期后使用 → 400 `INVALID_CAPTCHA` |
| A5 | 正确验证码 + 正确口令可登录 | → 200，含 token / tokenType / expiresAt / user |
| A6 | 验证码先于口令校验 | 用「错误口令 + 正确验证码」登录 → 401 `INVALID_CREDENTIALS`，且该验证码已失效 |
| A7 | 失败锁定 | 连续 5 次失败后再试 → 423 `ACCOUNT_LOCKED` |
| A8 | 前端登录页展示并提交验证码 | real 模式下提交请求体含 `captchaId`/`captchaCode`；登录失败后验证码自动刷新 |
| A9 | mock 模式可用 | mock 模式登录页展示验证码图片，错误验证码 → 「验证码错误或已失效」 |
| A10 | 既有能力不回归 | `bun test`、`bun run build`、`bun run lint` 全绿；后端 `cargo test` 全绿 |
| A11 | 凭据不入代码 | 仓库内检索不到任何账号口令字面量与固定 JWT 密钥；未配置 `RTS_AUTH_USERS` 时登录返回 401 `INVALID_CREDENTIALS` |
