# 开发计划（Plan）: web-ui 登录验证对接 rts-center

依据：`intents/20261031_ui-login-real-backend/spec.md`（已定稿）。本文件是「构建阶段」输出物。
**改动分支**：`feature/ui-login-real-backend`（worktree：`.claude/worktrees/ui-login-real-backend/`）。
**主仓 `main` 只读**：不切分支、不暂存、不提交。

## 0. 阶段总览与依赖

```
阶段 1 建表(DDL) ──┐
                    ├─▶ 阶段 3 令牌校验+管理接口 ──▶ 阶段 4 接口级测试 ──▶ 阶段 7 联调与验证
阶段 2 后端换源 ────┘                    ▲                                      ▲
                                                        阶段 6 前端对接 ──────────┘
                                                        阶段 5 引导 CLI ───────────┘
```

阶段 3~5 与阶段 6 可并行推进（不同文件），但阶段 4 的接口契约要先于阶段 6 定死（以 spec §3 为准）。

## 1. 改动清单

### 1.1 数据库

| 文件 | 动作 | 要点 |
|------|------|------|
| `scripts/database/sql/002_auth_user.sql` | 新增 | 由 **dba-toolkit** 技能产出：备份前置、幂等（`IF NOT EXISTS`）、每列 `COMMENT`、表 `COMMENT`；表名与列语义严格对齐 spec §2 |
| `docs/database/rts-schema.sql` | 修改 | 并入 `auth_user` 表定义（该文件当前为空） |
| `intents/.../plan.md` | 新增 | 本文件 |
| `intents/.../schema.sql` | 新增 | dba-toolkit 的迁移脚本落位处（若技能输出到 `scripts/database/sql/`，此处只做指针） |

DDL 纪律：**只有 CHG-001 一处变更**。实现中若发现还需动表结构 → 停下，回到 spec §2 加 CHGID 并重新评审。

### 1.2 后端 `rts-center`

| 文件 | 动作 | 要点 |
|------|------|------|
| `server/rts-center/Cargo.toml` | 修改 | 加 `sqlx = { version = "0.8", default-features = false, features = ["postgres", "runtime-tokio"] }`；**不开 `macros`**（编译期不连库） |
| `server/rts-center/src/config.rs` | 修改 | 删 `ENV_AUTH_USERS` / `load_accounts` / `AccountSpec` 相关；加 `ENV_DATABASE_URL`（必填）、`ENV_DB_MAX_CONNECTIONS`（默认 5，非法回退+WARN）；`Config::from_env` 缺 `DATABASE_URL` → 报错退出 |
| `server/rts-center/src/auth/users.rs` | 修改 | 保留 `hash_password` / `verify_password` / `find_user` 的哈希语义；`UserRecord` 增 `status`；新增 `AccountStore` trait（`async fn find_by_username`、`create`、`list`、`set_status`、`set_password`） |
| `server/rts-center/src/auth/db.rs` | 新增 | `PgAccountStore`：基于 `sqlx::PgPool` 的上述实现；SQL 全部用运行时查询（`sqlx::query` + `Row::try_get`），**不用编译期宏** |
| `server/rts-center/src/auth/bearer.rs` | 新增 | `Authorization: Bearer <token>` 提取器（Axum `FromRequestParts`）；缺头/格式错 → `TOKEN_INVALID` |
| `server/rts-center/src/auth/mod.rs` | 修改 | 登录改走 `AccountStore`；命中但 `status=disabled` → 403 `ACCOUNT_DISABLED`；新增 `me_handler`；保留"账号不存在也走一次 `verify_dummy_password`" |
| `server/rts-center/src/admin.rs` | 新增 | 4 个管理接口 + `super_admin` 鉴权；口令只入不出、日志只记审计三元组 |
| `server/rts-center/src/error.rs` | 修改 | 增 `TokenInvalid` / `AccountDisabled` / `Forbidden` / `NotFound` / `UsernameTaken` 及其状态码与文案 |
| `server/rts-center/src/state.rs` | 修改 | `AppState` 持 `PgPool` + `Arc<dyn AccountStore>`；构造器分开：生产 `new(config, pool)`，测试 `new_with_store(config, 内存 store)` |
| `server/rts-center/src/routes.rs` | 修改 | 挂载 `GET /api/v1/auth/me` 与 `/api/v1/admin/users` 系列 |
| `server/rts-center/src/main.rs` | 修改 | 建连接池（`PgPoolOptions`）；连不上 → 报错退出；`RTS_DB_MAX_CONNECTIONS` 生效 |
| `server/rts-center/src/lib.rs` | 修改 | 导出新模块（`admin`、`auth::db`、`auth::bearer`） |
| `server/rts-center/src/tests.rs` | 修改 | `build_app` 注入内存 `AccountStore`；补 spec §6 的接口级用例（AC-04~AC-13） |
| `server/rts-center/src/bin/rts-user.rs` | 新增 | 引导 CLI：`add`（口令从 stdin 读）/ `list`；与管理接口共用 `users` 服务层 |

### 1.3 前端 `web-ui`

| 文件 | 动作 | 要点 |
|------|------|------|
| `web-ui/src/api/auth/mock-auth-api.ts` | **删除** | 整文件 |
| `web-ui/src/api/auth/types.ts` | 修改 | `AuthApi` 增 `fetchMe(token): Promise<AuthUser>`；`AuthErrorCode` 增 `TOKEN_INVALID` / `FORBIDDEN` / `NOT_FOUND` / `USERNAME_TAKEN` |
| `web-ui/src/api/auth/http-auth-api.ts` | 修改 | 实现 `fetchMe`；`readErrorCode` 支持新码；错误体带 `message` 时优先用后端文案？——**否**，仍按本地码表映射（文案可控、不泄露后端内部信息） |
| `web-ui/src/api/auth/index.ts` | 修改 | 删 `VITE_API_MODE` 分支、`isMockMode`、`MOCK_CREDENTIALS` 导出；`authApi = new HttpAuthApi(baseUrl)` |
| `web-ui/src/api/auth/errors.ts` | 修改 | 文案表补 4 个新码 |
| `web-ui/src/auth/auth-provider.tsx` | 修改 | 挂载时 `fetchMe` 校验本地会话：200 → 刷新用户信息；401/403 → 清会话；网络错 → 保留 |
| `web-ui/src/components/login-form.tsx` | 修改 | 删「演示账号（mock 模式）」面板 |
| `web-ui/src/vite-env.d.ts` | 修改 | 删 `VITE_API_MODE`；加 `VITE_API_TARGET` |
| `web-ui/vite.config.ts` | 修改 | `/api` 代理默认目标 `http://127.0.0.1:8080`（`VITE_API_TARGET` 可覆盖） |
| `web-ui/Dockerfile` | 修改 | 删 `VITE_API_MODE` 构建参数 |
| `web-ui/tests/auth.test.ts` | 修改 | mock 用例改为打桩 `fetch` 的真实接口用例；会话恢复逻辑单测 |
| `web-ui/README.md` | 修改 | 环境变量表、mock 删除说明、联调步骤 |

### 1.4 文档

| 文件 | 动作 | 要点 |
|------|------|------|
| `server/rts-center/CLAUDE.md` | 修改 | 「凭据不入库」改述；`RTS_AUTH_USERS` 下线说明；新增模块表行；不变量新增「DB 故障 ≠ 认证失败」 |
| `server/rts-center/README.md` | 修改 | 环境变量表、迁移说明（先建表 + 开号）、启动示例 |
| `web-ui/README.md` | 修改 | 同上 |
| `intents/20261031_ui-login-real-backend/verify.md` | 新增 | 验证记录 |

## 2. 实施顺序（每阶段都要有可跑的验证）

| 阶段 | 内容 | 完成判据 |
|------|------|---------|
| **0 基线** | 在 worktree 分支跑 `cargo test --workspace`、`bun run test && bun run lint && bun run typecheck` | 两边基线全绿（不绿就先记为既有问题，不混进本次改动） |
| **1 建表** | dba-toolkit 产出 `002_auth_user.sql` → `db_caller.py` 在真库执行 → 并入 `rts-schema.sql` | 真库里 `auth_user` 存在，列/约束/注释符合 spec §2；重复执行幂等 |
| **2 依赖闸门** | 加 sqlx，跑 `cargo build -p rts-center` | **编译通过**。若镜像源/版本不兼容 → 立即回退方案：`tokio-postgres` + `deadpool-postgres`（同接口形状），不拖到后面才发现 |
| **3 后端换源** | `config` / `users` / `db` / `state` / `main` / 登录路径 | `cargo build` 通过；`cargo test -p rts-center` 全绿（用内存 store） |
| **4 令牌与管理接口** | `bearer` / `me_handler` / `admin` / `error` / `routes` | 新接口手工 curl 通；旧接口行为不变 |
| **5 接口级测试** | `tests.rs` 补 AC-04~AC-13 用例 | `cargo test -p rts-center` 全绿，且**在无数据库环境**下同样全绿 |
| **6 引导 CLI** | `src/bin/rts-user.rs` | 能建超管、能列账号；口令不出现在参数与日志里 |
| **7 前端对接** | 删 mock → api 层 → auth-provider → login-form → vite/Dockerfile → 测试 | `bun run test && bun run lint && bun run typecheck` 全绿；`grep` 确认产物无 mock 字面量 |
| **8 联调** | 真库起 rts-center → curl 闭环 → 浏览器 CDP 实测 → 容器 nginx 反代 | spec §6 的 AC-01~AC-18 逐条有实测记录 |
| **9 交付** | `verify.md` → 提交 → 推送 → 建 PR | PR 编号回填 `verify.md` |

## 3. 风险与对策

| # | 风险 | 影响 | 对策 |
|---|------|------|------|
| R1 | `sqlx` 在本机 registry（ustc 镜像）不可用或与 rustc 1.98 不兼容 | 阶段 2 编译失败 | 已核实镜像有 sqlx 0.8；阶段 2 设为**闸门**，失败即切 `tokio-postgres` + `deadpool-postgres`（接口形状不变） |
| R2 | `sqlx` 编译耗时显著拉长 | 构建变慢 | 只开 2 个 feature；不开 `macros`（避免 `DATABASE_URL` 参与编译） |
| R3 | 数据库网络/防火墙不稳定（库是远程实例，地址见仓库根 `.env`） | 服务起不来 / 登录 500 | 阶段 1 前先验连通（已验证可达）；启动失败给**明确**错误信息而不是静默 |
| R4 | 现有 48 个接口级测试因账号来源改造而大面积改动 | 返工 | 用 `AccountStore` 抽象隔离：测试换内存实现，业务断言基本不动 |
| R5 | 前端测试重写量大（`auth.test.ts` 半数用例依赖 mock） | 拖期 | 用 `fetch` 打桩替身，保留原有断言意图（验证码一次性、错误码映射等） |
| R6 | 浏览器实测受阻（WSL 无浏览器） | AC-16 无法验证 | 走 Windows Chrome headless + CDP（已有经验）；退化方案：curl + CDP 截图 |
| R7 | `RTS_AUTH_USERS` 下线是破坏性变更，存量部署起不来 | 运维中断 | 启动时缺 `DATABASE_URL` 直接报错退出（不静默降级）；README 写清迁移三步（建表 → 开超管 → 配 DATABASE_URL） |
| R8 | 口令出现在命令行/日志 | 凭据泄露 | CLI 口令走 stdin；日志只记审计三元组；提交前 `grep` 复查 |
| R9 | 停用语义与前端文案不匹配 | 体验割裂 | spec §3.4 已定 `ACCOUNT_DISABLED` 文案；前端错误表对齐 |

## 4. 验证方式（对应 spec §6）

| 类别 | 手段 |
|------|------|
| 后端单测 | `cargo test -p rts-center`（无需数据库）、`cargo test --workspace` |
| 后端静态检查 | `cargo clippy -p rts-center --all-targets`、`cargo fmt --all -- --check` |
| 前端 | `bun run test && bun run lint && bun run typecheck`（`web-ui/`） |
| 接口闭环 | 起真后端 + curl：「取码 → 登录 → /auth/me → 登出」，逐条记录 HTTP 状态与响应体 |
| 真库抽查 | `psql`/`db_caller.py` 查 `auth_user`：哈希前缀、状态、唯一性 |
| 浏览器实测 | Windows Chrome headless + CDP：登录成功 → 刷新仍登录 → 登出回登录页 → 错误口令提示 |
| 容器联调 | 前端镜像 + nginx 反代 `/api` → 后端，验证同源链路 |
| 交付检查 | `git diff` 复查无口令字面量；`grep -r` 确认 dist 无 mock 残留 |

## 5. 明确不做

- 用户管理前端页面（接口先备好，页面另起 intent）；
- 令牌刷新 / 撤销 / 黑名单；
- RBAC 权限表（角色字符串够用）；
- 验证码存储共享化（仍单实例内存态）；
- TLS / CORS（由反向代理负责）。
