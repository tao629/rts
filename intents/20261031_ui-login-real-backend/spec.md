# 需求规格说明书（Spec）: web-ui 登录验证对接 rts-center（数据库账号 + 令牌校验）

依据：`intents/20261031_ui-login-real-backend/intent.md`（状态 finalize）。本文件是「设计阶段」输出物，
由发起人评审后定稿；**改契约先改本文件**，代码只是本文件的实现。

## 1. 定位与边界

| 项 | 内容 |
|----|------|
| 名称 | UI 登录验证对接 rts-center |
| 涉及方 | `web-ui`（Vite + React SPA）、`rts-center`（axum 服务）、PostgreSQL `rts` 库 |
| 目标 | 登录链路上**没有任何一个环节由浏览器自己宣布成功**：验证码、口令校验、令牌发放、登录态确认全部由后端裁决；账号落在数据库里并可被管理 |
| 不做 | 用户管理前端页面；令牌刷新 / 撤销 / 黑名单；RBAC 权限表（角色字符串够用）；TLS 与 CORS（仍由反向代理负责）；验证码 OCR 对抗 |
| 部署形态 | 单实例 rts-center + 单个 PostgreSQL；前端由 nginx 托管并把 `/api` 反代到 rts-center |

### 1.1 目标链路

```
浏览器 ──GET /api/v1/auth/captcha──▶ rts-center ──▶ 返回 captchaId + 图片（答案留在服务端）
浏览器 ──POST /api/v1/auth/login───▶ rts-center ──▶ 校验验证码 → 查 auth_user → argon2id 校验 → 签发 JWT
浏览器 ──GET /api/v1/auth/me──────▶ rts-center ──▶ 验签 + 查库确认账号仍有效 → 返回用户信息
```

## 2. 数据模型变更清单（封闭清单）

> 按 dba-toolkit 规约，本节**不写 DDL**，只写语义与验收口径；实际 DDL 在构建阶段由
> `dba-toolkit` 产出到 `scripts/database/sql/002_auth_user.sql`，并并入 `docs/database/rts-schema.sql`。
> **没有 CHGID 就没有 schema 变更**；实现过程中若发现还需要动表结构，必须回到本节新增 CHGID 并重新评审。

| CHGID | 表 | 语义变更 | 回滚方式 |
|-------|-----|---------|---------|
| CHG-001 | `public.auth_user` | **新建账号表**。列语义见下表 | `DROP TABLE public.auth_user` |

`public.auth_user` 列语义（每列都写明类型、可空、取值域、谁写谁读、存量来源）：

| 列 | 类型 | 可空 | 取值域 / 默认 | 谁写入 | 谁读取 | 存量来源 |
|----|------|------|--------------|--------|--------|---------|
| `id` | `bigint` GENERATED ALWAYS AS IDENTITY | 否 | 系统生成，`u-` 前缀的对外 ID 不再使用，直接用数值 | 数据库 | 用户管理接口 | 新表，无存量 |
| `username` | `varchar(64)` | 否 | 唯一（**大小写不敏感**唯一）；仅允许 `[A-Za-z0-9._-]`，长度 3~64 | 创建接口 / 引导 CLI | 登录、管理接口 | 新表，无存量 |
| `password_hash` | `varchar(255)` | 否 | argon2id PHC 串（`$argon2id$v=19$…`）；**只允许哈希，不允许明文** | 创建接口 / 引导 CLI / 改密接口（服务端哈希后写入） | 登录校验 | 新表，无存量 |
| `display_name` | `varchar(64)` | 否 | 展示名，1~64 字符 | 创建接口 / 引导 CLI | 登录响应、`/auth/me`、管理接口 | 新表，无存量 |
| `roles` | `text[]` | 否 | 默认 `{}`（空数组）；元素为角色名（`super_admin` / `operator` / …），去重 | 创建接口 / 引导 CLI | 登录响应、`/auth/me`、管理接口鉴权 | 新表，无存量 |
| `status` | `varchar(16)` | 否 | 默认 `'active'`；取值域**只有** `'active'` 与 `'disabled'` | 创建接口（默认 active）/ 改状态接口 | 登录校验、`/auth/me` | 新表，无存量 |
| `created_at` | `timestamptz` | 否 | 默认 `now()`；只写一次 | 数据库 | 管理接口（审计展示） | 新表，无存量 |
| `updated_at` | `timestamptz` | 否 | 默认 `now()`；每次改状态 / 改口令时由服务端显式更新 | 服务端 | 管理接口（审计展示） | 新表，无存量 |

**怎么算做对了**（验收口径）：

1. 表建于 `public` schema，不新建 schema；
2. `username` 唯一约束生效：重复用户名（含大小写不同的重复）插入/开户失败；
3. `status` 只接受 `'active'` / `'disabled'`，非法值由服务端在写库前拦掉（不入库）；
4. `password_hash` 列里出现的每一行都以 `$argon2id$` 开头，**不存在任何明文口令**；
5. `roles` 为空数组时账号仍可登录，只是没有 `super_admin`，因而调不了管理接口。

## 3. 接口契约

服务基址 `/api/v1`。所有失败响应统一为 `{ "code": "<错误码>", "message": "<可读文案>" }`。

### 3.1 保持不变的接口

| 方法与路径 | 变化 |
|-----------|------|
| `GET /api/v1/auth/captcha` | 契约不变：返回 `{ captchaId, image, expiresAt }`，**答案绝不回传**；响应头 `Cache-Control: no-store` |
| `POST /api/v1/auth/login` | 请求体不变：`{ username, password, remember, captchaId, captchaCode }`；成功响应不变：`{ token, tokenType, expiresAt, user }`。**行为变化**：账号来源改为查库；账号被停用时返回 403 `ACCOUNT_DISABLED` |
| `POST /api/v1/auth/logout` | 契约不变：恒返回 204（服务端无黑名单），客户端本地清凭证 |

### 3.2 `GET /api/v1/auth/me`（新增）

| 项 | 内容 |
|----|------|
| 认证 | `Authorization: Bearer <token>` |
| 200 | `{ "id": "...", "username": "...", "displayName": "...", "roles": ["..."] }` —— 与登录响应的 `user` **同构**，前端可直接替换本地会话里的用户信息 |
| 401 `TOKEN_INVALID` | 令牌缺失、格式非 `Bearer`、签名不匹配、已过期 |
| 403 `ACCOUNT_DISABLED` | 令牌本身有效，但 `auth_user` 里该账号已被停用（停用即时生效） |
| 500 `SERVER_ERROR` | 数据库不可用（**不**降级成 401，避免把服务端故障说成"令牌无效"） |

### 3.3 账号管理接口（新增）

统一前置：**令牌有效**且 `roles` 含 `super_admin`，否则 401 `TOKEN_INVALID` / 403 `FORBIDDEN`。

| 方法与路径 | 请求 | 成功 | 失败 |
|-----------|------|------|------|
| `POST /api/v1/admin/users` | `{ username, password, displayName, roles?: ["..."] }` | 201 + `{ id, username, displayName, roles, status, createdAt }` | 400 `VALIDATION_ERROR`（字段非法）、409 `USERNAME_TAKEN` |
| `GET /api/v1/admin/users` | — | 200 + `{ "users": [ { id, username, displayName, roles, status, createdAt, updatedAt } ] }`（**不含哈希**） | — |
| `PATCH /api/v1/admin/users/{username}/status` | `{ status: "active" \| "disabled" }` | 200 + 该账号信息 | 400 `VALIDATION_ERROR`（状态值非法）、404 `NOT_FOUND` |
| `POST /api/v1/admin/users/{username}/password` | `{ password }` | 204 | 400 `VALIDATION_ERROR`、404 `NOT_FOUND` |

**共同约定**：

- 请求体里的 `password` 是明文，**只在这一次请求里出现**：服务端算完哈希即弃，
  不写日志、不回显、不进审计详情；
- 响应体与日志**永不出现口令与口令哈希**；
- 每个写操作记一条审计日志：操作者用户名、被操作账号、动作（create / disable / enable / reset-password）、
  结果；**不记口令**；
- 口令长度上限 128 字符（与登录侧 `MAX_PASSWORD_LEN` 一致），下限 8 字符。

### 3.4 错误码总表

| 错误码 | HTTP | 含义 | 前端文案 |
|--------|------|------|---------|
| `VALIDATION_ERROR` | 400 | 请求体字段非法 | 提交信息有误，请检查后重试 |
| `INVALID_CAPTCHA` | 400 | 验证码缺失 / 不匹配 / 过期 / 已使用 | 验证码错误或已失效，请重新输入 |
| `INVALID_CREDENTIALS` | 401 | 用户名或口令错误（**不区分账号是否存在**） | 用户名或密码错误 |
| `TOKEN_INVALID` | 401 | 令牌缺失 / 非法 / 过期 / 验签失败 | 登录状态已失效，请重新登录 |
| `ACCOUNT_DISABLED` | 403 | 账号已停用 | 该账号已被停用，请联系管理员 |
| `FORBIDDEN` | 403 | 已登录但无权限（非 `super_admin`） | 没有权限执行该操作 |
| `NOT_FOUND` | 404 | 目标账号不存在 | 目标账号不存在 |
| `USERNAME_TAKEN` | 409 | 用户名已存在 | 用户名已存在 |
| `ACCOUNT_LOCKED` | 423 | 登录失败次数过多，账号临时锁定 | 账号已锁定，请稍后再试或联系管理员 |
| `RATE_LIMITED` | 429 | 触发限流 | 尝试过于频繁，请稍后再试 |
| `SERVER_ERROR` | 500 | 服务端内部异常（含数据库不可用） | 服务器开小差了，请稍后再试 |

前端另有两个纯客户端错误码：`NETWORK_ERROR`（请求发不出去）、`VALIDATION_ERROR`（本地表单校验）。

### 3.5 认证方式小结

| 场景 | 凭证来源 | 校验者 |
|------|----------|--------|
| 取验证码 | 无（一次性挑战） | rts-center |
| 登录 | 用户名 + 口令 + captchaId + captchaCode | rts-center（查 `auth_user` + argon2id 校验） |
| 确认登录态 | Bearer 令牌 | rts-center（验签 + 查库确认账号仍启用） |
| 账号管理 | Bearer 令牌 + `super_admin` 角色 | rts-center |

## 4. 前端行为

### 4.1 删除项

- `web-ui/src/api/auth/mock-auth-api.ts`（整文件删除）与 `MOCK_CREDENTIALS`；
- `isMockMode` 与 `VITE_API_MODE` 开关（含 `vite-env.d.ts` 里的声明、Dockerfile 里的构建参数）；
- 登录页底部的「演示账号（mock 模式）」面板。

### 4.2 保留与新增

- `HttpAuthApi` 成为唯一的 `AuthApi` 实现；基础地址取 `VITE_API_BASE_URL`，默认 `/api/v1`（同源）。
- `AuthApi` 契约新增 `fetchMe(token): Promise<AuthUser>`，供会话恢复使用。
- 登录页流程不变：挂载即取验证码 → 提交登录 → 失败按错误码提示并自动换码重填。
- **会话恢复（新增行为）**：应用挂载时若本地存有会话，则调用 `GET /auth/me`：
  - 200 → 用后端返回的用户信息刷新本地会话（后端是唯一事实来源）；
  - 401 `TOKEN_INVALID` / 403 `ACCOUNT_DISABLED` → 清理本地凭证，视作未登录，由路由守卫弹回登录页；
  - 网络错误 → **保留**本地会话（网络抖动不该把人踢出登录态），但记录日志。
- 登出：调用 `POST /auth/logout`（失败不阻断），随后清理本地会话。

### 4.3 开发与部署配置

| 变量 | 作用 | 默认 |
|------|------|------|
| `VITE_API_BASE_URL` | 前端访问的 API 基址 | `/api/v1` |
| `VITE_API_TARGET` | **仅开发期**：vite dev server 把 `/api` 代理到的后端地址 | `http://127.0.0.1:8080` |
| `API_UPSTREAM` | **仅容器期**：nginx 把 `/api` 反代到的上游 | `http://host.docker.internal:8080` |
| `RTS_CENTER_ADDR` | rts-center 监听地址；容器部署需 `0.0.0.0:8080` | `127.0.0.1:8080` |

## 5. 配置与环境变量（后端）

| 变量 | 必填 | 默认 | 说明 |
|------|------|------|------|
| `DATABASE_URL` | **是** | — | PostgreSQL 连接串；缺失或连不上 → **启动报错退出**，不带着半截配置跑起来 |
| `RTS_DB_MAX_CONNECTIONS` | 否 | `5` | 连接池上限；非法值 → 回退默认 + WARN |
| `RTS_JWT_SECRET` | 否 | 随机（临时） | 令牌签名密钥；不配置则进程重启后旧令牌失效 |
| `RTS_CENTER_ADDR` | 否 | `127.0.0.1:8080` | 监听地址；非法 → 启动报错退出 |
| `RTS_CAPTCHA_TTL_SECS` | 否 | `300` | 验证码有效期 |
| `RTS_LOGIN_MAX_FAILURES` | 否 | `5` | 触发锁定的连续失败次数 |
| `RTS_LOGIN_LOCK_SECS` | 否 | `60` | 锁定期时长 |
| ~~`RTS_AUTH_USERS`~~ | — | — | **下线**：账号不再由环境变量注入 |

## 6. 验收标准

| 编号 | 标准 | 验证方式 |
|------|------|---------|
| AC-01 | 前端构建产物里搜不到 `mock-token-`、`Admin@123`、`Operator@123` | `grep -r` `web-ui/dist/assets` |
| AC-02 | 未设 `DATABASE_URL` 时 rts-center 启动即报错退出 | 起进程看退出码与 stderr |
| AC-03 | `GET /api/v1/auth/captcha` 返回三项字段，图片是 base64 data URI，响应体无答案，响应头含 `Cache-Control: no-store` | curl + 接口级测试 |
| AC-04 | 正确验证码 + 正确口令 → 200，返回 `token` / `tokenType=Bearer` / `expiresAt` / `user` | 接口级测试 + curl 闭环 |
| AC-05 | 口令错误 → 401 `INVALID_CREDENTIALS`，且该验证码随即作废（重放同 captchaId → 400） | 接口级测试 |
| AC-06 | 验证码错误 / 缺失 → 400 `INVALID_CAPTCHA` | 接口级测试 |
| AC-07 | 连续失败达阈值 → 下一次 423 `ACCOUNT_LOCKED` | 接口级测试 |
| AC-08 | 停用账号登录 → 403 `ACCOUNT_DISABLED` | 接口级测试 |
| AC-09 | `/auth/me` 无令牌 → 401 `TOKEN_INVALID`；带有效令牌 → 200 返回用户信息 | 接口级测试 |
| AC-10 | 停用账号的令牌调 `/auth/me` → 403 `ACCOUNT_DISABLED` | 接口级测试 |
| AC-11 | 非 `super_admin` 调 `/api/v1/admin/users` → 403 `FORBIDDEN` | 接口级测试 |
| AC-12 | `super_admin` 调 `POST /api/v1/admin/users` → 201，库里新增一行且 `password_hash` 以 `$argon2id$` 开头 | 接口级测试 + 真库抽查 |
| AC-13 | 用户名重复 → 409 `USERNAME_TAKEN` | 接口级测试 |
| AC-14 | `cargo test --workspace` 全绿，且**不依赖真实数据库** | 本机执行 |
| AC-15 | `bun run test && bun run lint && bun run typecheck` 全绿 | `web-ui/` 执行 |
| AC-16 | 浏览器实测：真实后端登录成功 → 刷新页面仍为登录态 → 登出后回到登录页 | Windows Chrome headless + CDP |
| AC-17 | rts-center 日志能查到本次登录成功 / 失败记录，且**不含口令** | 看日志文件 |
| AC-18 | `rts-center` 的 `/api/v1/*` 全部接口在 nginx 反代下可达（前端容器 → 后端） | 容器联调 |

## 7. 与既有约束的冲突与取舍

| 冲突点 | 处理 |
|--------|------|
| `server/rts-center/CLAUDE.md` 写着「凭据不入库」 | **不违反**：入库的是 argon2id 哈希，不是明文口令；该文档相应改写为「口令不入库、不入库的是哈希」 |
| 登录路径新增了数据库依赖 | 数据库不可用 → 登录返回 500 而非 401；**不接受**"查不到就当口令错"的降级（会把故障说成认证失败） |
| 账号枚举防护（账号不存在也走一次等价 argon2） | 保留：查库未命中时仍执行一次 `verify_dummy_password`，抹平耗时差异 |
| 验证码存储仍是单进程内存 | 不变（本次不引入共享存储）；因此 rts-center 仍按**单实例**部署，账号表是共享的、验证码不是 |
| 引入 `sqlx` 依赖 | 只取 `postgres` + `runtime-tokio` 两个 feature，**不开 `macros`**——避免编译期需要数据库连接串（CI/离线构建友好）；编译时间与依赖面增加是可接受代价 |
| 后端接口测试不能依赖真实数据库 | 账号读取走 `AccountStore` 抽象：生产实现查库，测试实现为内存替身 |
| `RTS_AUTH_USERS` 下线 | 属于**破坏性变更**，在 `server/rts-center/README.md` 与 `CLAUDE.md` 的迁移说明里写清楚 |
