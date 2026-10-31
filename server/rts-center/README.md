# rts-center

搬运工平台的服务中心，对外提供 HTTP 接口（基址 `/api/v1`），当前包含
**图形验证码 + 登录 / 登出 / 确认登录态 / 账号管理 / 健康检查**。

契约来源：[`intents/20261031_ui-login-real-backend/spec.md`](../../intents/20261031_ui-login-real-backend/spec.md)。

---

## 1. 启动

**账号在数据库里**，所以启动前要先建表、再开一个超管账号：

```bash
# ① 建表（仓库根目录执行，幂等）
uv run --directory scripts/database db_caller.py sql/002_auth_user.sql

# ② 开第一个超管（口令从标准输入读，不进命令行历史）
export DATABASE_URL='postgresql://用户:口令@主机:5432/库名'
printf '%s\n' '你的口令' | cargo run -p rts-center --bin rts-user -- \
  add --username admin --display-name 系统管理员 --roles super_admin

# ③ 起服务
export RTS_JWT_SECRET="$(openssl rand -hex 32)"
cargo run -p rts-center
```

或者先构建再运行：

```bash
cargo build --release -p rts-center
./target/release/rts-center
```

**凭据一律由环境变量注入，仓库里不存放任何口令或签名密钥。**
数据库里存的也只是口令的 argon2id 哈希，不存明文。

---

## 2. 环境变量

| 变量 | 默认值 | 说明 |
|------|--------|------|
| `DATABASE_URL` | **必填** | PostgreSQL 连接串（账号表所在库）。缺失或为空 → **启动报错退出** |
| `RTS_DB_MAX_CONNECTIONS` | `5` | 数据库连接池上限 |
| `RTS_CENTER_ADDR` | `127.0.0.1:8080` | 监听地址。默认只监听回环地址，对外暴露请放在反向代理之后 |
| `RTS_JWT_SECRET` | 无 | 令牌签名密钥（HS256）。**未配置时随机生成一个 32 字节密钥**，并打 WARN：令牌在进程重启后即失效 |
| `RTS_CAPTCHA_TTL_SECS` | `300` | 图形验证码有效期（秒） |
| `RTS_LOGIN_MAX_FAILURES` | `5` | 触发锁定的连续失败次数 |
| `RTS_LOGIN_LOCK_SECS` | `60` | 锁定时长（秒） |

取值非法（非正整数）的数值型变量会回退到默认值并打 WARN，不会导致启动失败；
`RTS_CENTER_ADDR` 解析失败、`DATABASE_URL` 缺失会**直接报错退出**。

> **迁移说明（破坏性变更）**：账号原先由环境变量 `RTS_AUTH_USERS` 注入，现已下线。
> 旧部署需要三步迁移：① 建表；② 用 `rts-user` 开超管；③ 配置 `DATABASE_URL`。

日志沿用平台公共包 `rts-core`：`RTS_LOG_DIR` / `RTS_LOG_LEVEL` / `RTS_LOG_CONSOLE` 等见
[`base/rts-core`](../../base/rts-core)。

---

## 3. 账号表 `auth_user`

表结构见 [`docs/database/rts-schema.sql`](../../docs/database/rts-schema.sql)，
迁移脚本 `scripts/database/sql/002_auth_user.sql`（幂等）。

| 列 | 说明 |
|----|------|
| `id` | 主键，数据库生成 |
| `username` | 登录名，`[A-Za-z0-9._-]`，3~64 字符，**大小写不敏感唯一** |
| `password_hash` | argon2id PHC 串，**只写哈希** |
| `display_name` | 显示名，1~64 字符 |
| `roles` | 角色名数组；含 `super_admin` 才可调用账号管理接口 |
| `status` | `active` / `disabled`；停用后登录被拒、已签发令牌随即失效 |
| `created_at` / `updated_at` | 建号时间 / 最后变更时间 |

登录时按用户名查库（走唯一索引），用库里的哈希做 argon2id 校验。
**账号不存在时也会走一次等价的 argon2 运算**，抹平"账号不存在"与"口令错误"的耗时差异。

### 3.1 `rts-user` 引导与救援工具

账号管理接口本身要求 `super_admin` 令牌，库里一个超管都没有时它调不了——
所以留了这个命令行入口。它与管理接口共用同一套账号逻辑。

```bash
# 新增账号（口令从标准输入读）
printf '%s\n' '你的口令' | cargo run -p rts-center --bin rts-user -- \
  add --username alice --display-name 爱丽丝 --roles operator

# 列出账号（不回显口令哈希）
cargo run -p rts-center --bin rts-user -- list

# 停用 / 启用 / 重置口令
cargo run -p rts-center --bin rts-user -- disable --username alice
cargo run -p rts-center --bin rts-user -- enable  --username alice
printf '%s\n' '新口令' | cargo run -p rts-center --bin rts-user -- \
  reset-password --username alice
```

口令**只从标准输入读**，不接受命令行参数——命令行会进 shell 历史。

---

## 4. 接口清单

所有响应 `Content-Type: application/json`（登出与改口令为 204 空响应）。
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
    "id": "1",
    "username": "admin",
    "displayName": "系统管理员",
    "roles": ["super_admin"]
  }
}
```

**校验顺序固定为：验证码 → 口令 → 失败锁定。**
即便口令错误，验证码也已被消费，客户端必须重新取码。
账号不存在与口令错误返回同一个错误码，避免被用来枚举账号。
账号被停用且口令正确时返回 403 `ACCOUNT_DISABLED`（口令错误时仍返回
`INVALID_CREDENTIALS`，避免靠这个错误码探测账号状态）。

### 4.3 `GET /api/v1/auth/me` — 确认登录态

请求头 `Authorization: Bearer <token>`。用令牌换回当前用户信息，
前端在刷新页面时用它确认登录态仍然有效——**登录态由服务端裁决**。

```json
{
  "id": "1",
  "username": "admin",
  "displayName": "系统管理员",
  "roles": ["super_admin"]
}
```

| 情形 | 响应 |
|------|------|
| 令牌有效 | 200 + 上面的用户信息 |
| 令牌缺失 / 非法 / 过期 / 验签失败 | 401 `TOKEN_INVALID` |
| 账号已停用 | 403 `ACCOUNT_DISABLED` |
| 数据库不可用 | 500 `SERVER_ERROR`（**不**降级成 401：服务端故障不是认证失败） |

### 4.4 `POST /api/v1/auth/logout` — 登出

请求头 `Authorization: Bearer <token>`，恒返回 `204` 空响应。
服务端不维护令牌黑名单，客户端本地清理凭证即可。

### 4.5 账号管理接口

全部要求 `Authorization: Bearer <token>` 且令牌 `roles` 含 `super_admin`。

| 方法与路径 | 请求 | 成功 | 失败 |
|-----------|------|------|------|
| `POST /api/v1/admin/users` | `{ username, password, displayName, roles? }` | 201 + 账号信息 | 400 `VALIDATION_ERROR`、409 `USERNAME_TAKEN` |
| `GET /api/v1/admin/users` | — | 200 + `{ "users": [...] }` | — |
| `PATCH /api/v1/admin/users/{username}/status` | `{ status: "active" \| "disabled" }` | 200 + 账号信息 | 400 `VALIDATION_ERROR`、404 `NOT_FOUND` |
| `POST /api/v1/admin/users/{username}/password` | `{ password }` | 204 | 400 `VALIDATION_ERROR`、404 `NOT_FOUND` |

约定：

- 请求体里的明文口令**只在这一次请求里出现**：服务端算完哈希即弃，
  不写日志、不回显、不进审计详情；
- 响应体与日志**永不出现口令与口令哈希**；
- 每个写操作记一条审计日志：操作者、被操作账号、动作、结果，**不记口令**；
- 口令长度 8~128 字符，用户名 3~64 字符，显示名 1~64 字符，角色名 ≤ 32 字符、最多 16 个。

### 4.6 `GET /api/v1/health` — 健康检查

```json
{ "status": "ok", "time": "2026-10-25T02:14:30Z" }
```

---

## 5. 错误码

| 错误码 | HTTP | 触发条件 |
|--------|------|----------|
| `VALIDATION_ERROR` | 400 | 用户名/口令缺失、为空、超长，或请求体非法 |
| `INVALID_CAPTCHA` | 400 | 验证码缺失 / 不匹配 / 过期 / 已使用 |
| `INVALID_CREDENTIALS` | 401 | 用户名或口令错误（含"账号不存在"的情形） |
| `TOKEN_INVALID` | 401 | 令牌缺失 / 非法 / 过期 / 验签失败 |
| `ACCOUNT_DISABLED` | 403 | 账号已停用（登录与 `/auth/me` 都会命中） |
| `FORBIDDEN` | 403 | 已登录但不是 `super_admin` |
| `NOT_FOUND` | 404 | 目标账号不存在 |
| `USERNAME_TAKEN` | 409 | 用户名已存在 |
| `ACCOUNT_LOCKED` | 423 | 同一「用户名 + 来源 IP」连续失败达阈值，锁定期内再次登录 |
| `RATE_LIMITED` | 429 | 触发限流（本次与账号锁定同源，保留该码以兼容前端既有枚举） |
| `SERVER_ERROR` | 500 | 服务端内部异常（**含数据库不可用**） |

---

## 6. 部署注意

- **数据库是登录路径的依赖**：查不到账号时返回 500 而不是伪装成认证失败——
  把服务端故障说成"口令错误"会让运维分不清是库挂了还是口令错了。
- **多实例部署**：验证码存储、失败计数都在**单进程内存**里，多实例之间不共享，
  验证码需要"取码与登录落在同一实例"。本次不引入 Redis 等外部依赖，
  需要水平扩展时必须先替换成共享存储。账号表本身是共享的。
- **反向代理**：来源 IP 优先取 `X-Forwarded-For` / `X-Real-IP`，其次取 TCP 对端地址。
  没有可信代理白名单时转发头可被伪造，它只作锁定维度的辅助信息。
- **令牌**：HS256 对称签名，`RTS_JWT_SECRET` 生产必须覆盖；不配置时令牌在进程重启后失效。
  当前没有令牌撤销 / 刷新机制，敏感操作需要额外的令牌校验。
- **HTTPS**：服务端只提供明文 HTTP，对外暴露时请由反向代理终结 TLS。
- **CORS**：未实现 CORS 头，前端需要同源访问或由代理处理跨域。
