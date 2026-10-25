# rts-center

搬运工平台的服务中心，对外提供 HTTP 接口（基址 `/api/v1`），当前包含
**图形验证码 + 登录 / 登出 / 健康检查**。

契约来源：[`sdlc/20261025_登录验证码/spec.md`](../../sdlc/20261025_登录验证码/spec.md)。

---

## 1. 启动

```bash
# 工作区根目录执行
cargo run -p rts-center
```

或者先构建再运行：

```bash
cargo build --release -p rts-center
./target/release/rts-center
```

**凭据一律由环境变量注入，仓库里不存放任何口令或签名密钥。**
未配置账号时服务照常启动，但所有登录都会失败（启动日志里有 WARN 提示）。

最小可用启动示例（请把 `<口令>` 换成部署方自己的口令）：

```bash
export RTS_AUTH_USERS='admin:<口令>:系统管理员:super_admin;operator:<口令>:值班运维:operator'
export RTS_JWT_SECRET="$(openssl rand -hex 32)"
cargo run -p rts-center
```

---

## 2. 环境变量

| 变量 | 默认值 | 说明 |
|------|--------|------|
| `RTS_CENTER_ADDR` | `127.0.0.1:8080` | 监听地址。默认只监听回环地址，对外暴露请放在反向代理之后 |
| `RTS_JWT_SECRET` | 无 | 令牌签名密钥（HS256）。**未配置时随机生成一个 32 字节密钥**，并打 WARN：令牌在进程重启后即失效 |
| `RTS_CAPTCHA_TTL_SECS` | `300` | 图形验证码有效期（秒） |
| `RTS_LOGIN_MAX_FAILURES` | `5` | 触发锁定的连续失败次数 |
| `RTS_LOGIN_LOCK_SECS` | `60` | 锁定时长（秒） |
| `RTS_AUTH_USERS` | 无（账号表为空） | 账号表，格式见下文 |

取值非法（非正整数）的数值型变量会回退到默认值并打 WARN，不会导致启动失败；
`RTS_CENTER_ADDR` 解析失败、`RTS_AUTH_USERS` 格式非法会**直接报错退出**。

日志沿用平台公共包 `rts-core`：`RTS_LOG_DIR` / `RTS_LOG_LEVEL` / `RTS_LOG_CONSOLE` 等见
[`base/rts-core`](../../base/rts-core)。

---

## 3. `RTS_AUTH_USERS` 账号配置格式

```
<用户名>:<口令>:<显示名>[:<角色1>,<角色2>][;<下一个账号>...]
```

示例（`<口令>` 是占位符，请替换为真实口令）：

```bash
export RTS_AUTH_USERS='admin:<口令>:系统管理员:super_admin;operator:<口令>:值班运维:operator'
```

解析规则：

| 规则 | 说明 |
|------|------|
| 账号分隔 | 多个账号用 `;` 分隔 |
| 字段分隔 | 单个账号内用 `:` 分隔，依次为 `用户名` / `口令` / `显示名` / `角色列表` |
| 字段个数 | 必须是 3 或 4 个：角色列表可省略（该账号无角色），也可用空串占位 |
| 角色列表 | 多个角色用 `,` 分隔；空角色元素会被忽略 |
| 口令字符 | **不得包含 `:` 与 `;`**（会被当作分隔符）。口令按字面量取用，首尾空格有效 |
| 空条目 | `;;`、结尾多一个 `;` 造成的空条目视为非法 |
| 用户名重复 | 视为非法 |

**非法输入一律不静默跳过**：启动时打印明确的错误（指出第几个条目、哪里不对）并以非零码退出。
错误信息里**不会**包含口令。

口令的处理方式：启动时对注入的明文口令做一次 argon2id 哈希，内存里只保留 PHC 哈希串；
校验时用 argon2 verify。**服务不会把口令写进日志。**

未配置 `RTS_AUTH_USERS`（未设置或为空串）时：账号表为空，服务正常启动，
启动日志打 WARN「未配置账号，所有登录都会失败」，任何登录请求都返回 `INVALID_CREDENTIALS`。

---

## 4. 接口清单

所有响应 `Content-Type: application/json`（登出为 204 空响应）。
错误响应统一为 `{ "code": "<错误码>", "message": "<可读文案>" }`。

### 4.1 `GET /api/v1/auth/captcha` — 获取图形验证码

响应头带 `Cache-Control: no-store`。

```json
{
  "captchaId": "b3f1c0d28a414f0e9c7b1d2e3f4a5b6c",
  "image": "data:image/svg+xml;base64,PHN2ZyB4bWxucz0i...",
  "expiresAt": "2026-10-25T02:15:30Z"
}
```

- `image` 可直接用于 `<img src>`；
- 验证码答案**不会**出现在响应里；
- 验证码 4 位，字符集 `23456789ABCDEFGHJKMNPQRSTUVWXYZ`，大小写不敏感；
- **一次性**：任何一次校验尝试（成功或失败）后立即失效。

### 4.2 `POST /api/v1/auth/login` — 登录

```json
{
  "username": "admin",
  "password": "<口令>",
  "remember": true,
  "captchaId": "b3f1c0d28a414f0e9c7b1d2e3f4a5b6c",
  "captchaCode": "8F3K"
}
```

| 字段 | 必填 | 说明 |
|------|------|------|
| `username` | 是 | 去首尾空格后非空，长度 ≤ 64 |
| `password` | 是 | 非空，长度 ≤ 128 |
| `remember` | 否 | 缺省 `false`；`true` → 令牌 24 小时，`false` → 2 小时 |
| `captchaId` | 是 | 缺失或为空即拒绝 |
| `captchaCode` | 是 | 缺失或为空即拒绝；大小写不敏感 |

成功响应：

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

**校验顺序固定为：验证码 → 口令 → 失败锁定。**
即便口令错误，验证码也已被消费，客户端必须重新取码。
账号不存在与口令错误返回同一个错误码，避免被用来枚举账号。

### 4.3 `POST /api/v1/auth/logout` — 登出

请求头 `Authorization: Bearer <token>`，恒返回 `204` 空响应。
服务端不维护令牌黑名单，客户端本地清理凭证即可。

### 4.4 `GET /api/v1/health` — 健康检查

```json
{ "status": "ok", "time": "2026-10-25T02:14:30Z" }
```

---

## 5. 错误码

| 错误码 | HTTP | 触发条件 |
|--------|------|----------|
| `VALIDATION_ERROR` | 400 | 用户名/口令缺失、为空、超长，或请求体非法 |
| `INVALID_CAPTCHA` | 400 | 验证码缺失 / 不匹配 / 过期 / 已使用 |
| `INVALID_CREDENTIALS` | 401 | 用户名或口令错误（含"未配置账号"的情形） |
| `ACCOUNT_LOCKED` | 423 | 同一「用户名 + 来源 IP」连续失败达阈值，锁定期内再次登录 |
| `RATE_LIMITED` | 429 | 触发限流（本次与账号锁定同源，保留该码以兼容前端既有枚举） |
| `SERVER_ERROR` | 500 | 服务端内部异常 |

---

## 6. 部署注意

- **多实例部署**：验证码存储、失败计数都在**单进程内存**里，多实例之间不共享，
  验证码需要"取码与登录落在同一实例"。本次不引入 Redis 等外部依赖，
  需要水平扩展时必须先替换成共享存储。
- **反向代理**：来源 IP 优先取 `X-Forwarded-For` / `X-Real-IP`，其次取 TCP 对端地址。
  没有可信代理白名单时转发头可被伪造，它只作锁定维度的辅助信息。
- **令牌**：HS256 对称签名，`RTS_JWT_SECRET` 生产必须覆盖；不配置时令牌在进程重启后失效。
  当前没有令牌撤销 / 刷新机制，敏感操作需要额外的令牌校验。
- **HTTPS**：服务端只提供明文 HTTP，对外暴露时请由反向代理终结 TLS。
- **CORS**：未实现 CORS 头，前端需要同源访问或由代理处理跨域。
