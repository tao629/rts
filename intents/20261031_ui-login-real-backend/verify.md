# 验证记录（Verify）: web-ui 登录验证对接 rts-center（数据库账号 + 令牌校验）

依据：`intents/20261031_ui-login-real-backend/spec.md`（定稿）、`plan.md`（9 阶段）。
分支：`feature/ui-login-real-backend`（worktree `.claude/worktrees/ui-login-real-backend/`）。

## 0. 验证环境

| 项 | 值 |
|----|-----|
| 主机 | WSL2（Linux 6.18.33.2，x86_64） |
| Rust | rustc 1.98.1 / cargo 1.98.1 |
| 前端 | bun 1.3.14 / Vite 8.3.5 / React 19 |
| 数据库 | PostgreSQL 18.6（远程实例，连接串见仓库根 `.env`；此处不落地址与口令），schema `public` |
| 浏览器 | Windows Chrome（headless=new）+ CDP，CDP 只绑 `127.0.0.1`（WSL 与 Windows 共享 loopback） |
| 凭据 | 全部**运行时随机生成**（`secrets`），只经 stdin 交给 `rts-user`，不落盘、不进命令行、不打印 |

---

## 1. 逐条验收标准对照

| 编号 | 标准 | 结果 | 实测数据 / 方式 |
|------|------|------|-----------------|
| AC-01 | 前端产物里搜不到 mock 口令字面量 | ✅ | `grep -rl "mock-token\|Admin@123\|Operator@123\|MOCK_CREDENTIALS\|MockAuthApi" web-ui/dist/` → 无输出；产物 `dist/assets/index-ClXHqL8Z.js` 451.86 kB |
| AC-02 | 未设 `DATABASE_URL` 时启动即报错退出 | ✅ | `env -u DATABASE_URL ./target/debug/rts-center` → `配置项 DATABASE_URL 无效: 未配置数据库连接串，账号表无法加载`，退出码 `1` |
| AC-03 | 验证码接口返回三项字段、base64 data URI、响应头 no-store | ✅ | 接口级用例 `captcha_endpoint_returns_three_fields_with_data_uri` / `captcha_response_has_no_store_cache_control`；真库 curl 复核见 §2 |
| AC-04 | 正确验证码 + 正确口令 → 200，返回 token/user | ✅ | 用例 `login_with_correct_captcha_and_password_succeeds`；真库端到端见 §2 |
| AC-05 | 口令错误 → 401 `INVALID_CREDENTIALS`，验证码随即作废 | ✅ | 用例 `wrong_password_returns_401_and_consumes_captcha`；真库端到端见 §2 |
| AC-06 | 验证码错误 / 缺失 → 400 `INVALID_CAPTCHA` | ✅ | 用例 `login_with_wrong_captcha_returns_invalid_captcha`、`login_without_captcha_returns_invalid_captcha`、`login_with_missing_captcha_fields_returns_invalid_captcha` |
| AC-07 | 连续失败达阈值 → 下一次 423 `ACCOUNT_LOCKED` | ✅ | 用例 `account_is_locked_after_max_failures`（阈值 3，第 4 次起 423；锁定期内即便口令正确也 423） |
| AC-08 | 停用账号登录 → 403 `ACCOUNT_DISABLED` | ✅ | 用例 `disabled_account_cannot_login`；真库端到端见 §2。补充：口令错误时仍返回 `INVALID_CREDENTIALS`（用例 `disabled_account_with_wrong_password_still_reports_invalid_credentials`），避免靠错误码探测账号状态 |
| AC-09 | `/auth/me` 无令牌 → 401 `TOKEN_INVALID`；带有效令牌 → 200 用户信息 | ✅ | 用例 `me_endpoint_returns_user_info_for_valid_token` / `me_endpoint_without_token_returns_token_invalid` / `me_endpoint_with_forged_token_returns_token_invalid` |
| AC-10 | 停用账号的令牌调 `/auth/me` → 403 `ACCOUNT_DISABLED` | ✅ | 用例 `me_endpoint_rejects_disabled_account` |
| AC-11 | 非 `super_admin` 调管理接口 → 403 `FORBIDDEN` | ✅ | 用例 `admin_endpoint_rejects_non_admin_role` |
| AC-12 | 超管建号 → 201，库里 `password_hash` 以 `$argon2id$` 开头 | ✅ | 用例 `admin_can_create_user_with_argon2_hash`；真库抽查见 §2 |
| AC-13 | 用户名重复 → 409 `USERNAME_TAKEN` | ✅ | 用例 `creating_duplicated_username_returns_conflict`；真库端到端见 §2 |
| AC-14 | `cargo test --workspace` 全绿，**不依赖真实数据库** | ✅ | 见 §3；接口级测试注入内存 `AccountStore`，全程未连库 |
| AC-15 | `bun run test && bun run lint && bun run typecheck` 全绿 | ✅ | 见 §3 |
| AC-16 | 浏览器实测：登录成功 → 刷新仍登录 → 登出回登录页 | ✅ | 见 §4（Windows Chrome headless + CDP，8 项全通过） |
| AC-17 | rts-center 日志能查到登录成功/失败记录，且不含口令 | ✅ | 见 §5 |
| AC-18 | 容器 nginx 反代下 `/api/v1/*` 可达 | ⚠️ **未验证** | 本机无 Docker：WSL 侧无 `docker` 命令，Windows 侧也没有 Docker Desktop（`/mnt/c/Program Files/Docker` 不存在）。等价路径已实测：vite dev server 的同源 `/api` 代理走通完整登录闭环（§4）；nginx 模板与 Dockerfile 本次只删掉一个构建参数 `VITE_API_MODE`，反代逻辑未动 |

---

## 2. 后端真库端到端（curl 等价：直接打 HTTP 接口）

脚本：`/tmp/rts_auth_e2e_20261031.py`（临时脚本，不入库）。真库 `rts` 库，端口 `127.0.0.1:18081`。
脚本自建随机命名账号（`e2e_root_<hex>` / `e2e_<hex>`），结束后**按精确用户名**逐个删除。

```
[PASS] rts-user 建号 :: 已创建账号 e2e_root_9058f9bb（id=11，角色：super_admin）
[PASS] rts-center 启动并连上数据库
[PASS] GET /auth/captcha 返回三项字段 :: status=200
[PASS] 正确口令登录成功 :: status=200 code=None
[PASS] GET /auth/me 用令牌换回用户信息 :: status=200 body={'id': '11', 'username': 'e2e_root_9058f9bb', 'displayName': '端到端测试超管', 'roles': ['super_admin']}
[PASS] 错误口令返回 401 INVALID_CREDENTIALS :: status=401 code=INVALID_CREDENTIALS
[PASS] /auth/me 无令牌返回 401 TOKEN_INVALID :: status=401 code=TOKEN_INVALID
[PASS] 超管新增账号返回 201 :: status=201 body={'id': '12', 'username': 'e2e_292839db', 'displayName': '端到端测试账号', 'roles': ['operator'], 'status': 'active', 'createdAt': '2026-10-31T11:38:05Z', 'updatedAt': '2026-10-31T11:38:05Z'}
[PASS] 库里存的是 argon2id 哈希且不含明文 :: prefix=$argon2id$v=...
[PASS] 重复用户名返回 409 USERNAME_TAKEN :: status=409 code=USERNAME_TAKEN
[PASS] 新账号能登录 :: status=200
[PASS] 超管停用账号返回 200 :: status=200
[PASS] 停用账号登录返回 403 ACCOUNT_DISABLED :: status=403 code=ACCOUNT_DISABLED
[PASS] 账号列表不含口令哈希 :: status=200
[PASS] 管理接口无令牌返回 401 TOKEN_INVALID :: status=401 code=TOKEN_INVALID
[INFO] 已清理测试账号 e2e_root_9058f9bb
[INFO] 已清理测试账号 e2e_292839db

全部通过
```

### 2.1 建表与约束（真库）

迁移：`uv run --directory scripts/database db_caller.py sql/002_auth_user.sql`

```
▶ 执行 sql/002_auth_user.sql （sha256 09962a3faa3f，11 条语句）
  无需备份（涉及的表尚不存在）
  [1] 行 11  CREATE TABLE IF NOT EXISTS auth_user (    32ms
  ...（8 条 COMMENT ON）
  [11] 行 50  CREATE UNIQUE INDEX IF NOT EXISTS auth_user_username_lower_key    26ms
✔ sql/002_auth_user.sql 提交成功（11 条语句，总耗时 306ms）
```

幂等复跑：

```
▶ 跳过 sql/002_auth_user.sql （已执行过，sha256 09962a3faa3f，记录于 2026-10-31T10:41:32Z）
```

结构与约束抽查（真实库）：

| 检查 | 实测 |
|------|------|
| `id` 是 identity 列 | `is_identity='YES'`, `identity_generation='ALWAYS'` |
| `roles` 默认值 | `'{}'::text[]` |
| `status` 默认值 | `'active'::character varying` |
| `status` 取值域 | `CHECK (status IN ('active','disabled'))` → 插入 `'unknown'` 被拒（CheckViolation） |
| 用户名格式 | `CHECK (username ~ '^[A-Za-z0-9._-]{3,64}$')` → 插入 `'ab'` 被拒 |
| 显示名非空 | `CHECK (length(btrim(display_name)) BETWEEN 1 AND 64)` → 插入 `''` 被拒 |
| **大小写不敏感唯一** | 先插 `'Admin'` 再插 `'admin'` → `UniqueViolation: duplicate key value violates unique constraint "auth_user_username_lower_key"` |
| 建表后账号表为空 | `select count(*) from auth_user` → `0` |

备份：操作前结构快照已写入 `scripts/database/backup/db-schema.bak`（含操作前 `public` schema
的列/约束/索引内省快照；该目录被 `.gitignore` 忽略，不入库）。

---

## 3. 静态检查与测试

### 后端

```
$ cargo test --workspace
     Running unittests src/main.rs (f_matcher)      test result: ok. 53 passed; 0 failed
     Running unittests src/main.rs (r_connector)    test result: ok. 27 passed; 0 failed
     Running unittests src/lib.rs (rts_center)      test result: ok. 62 passed; 0 failed
     Running unittests src/lib.rs (rts_core)        test result: ok. 16 passed; 0 failed
     Running tests/init.rs                           test result: ok.  1 passed; 0 failed
     Doc-tests rts_center                           test result: ok.  0 passed; 0 failed
     Doc-tests rts_core                             test result: ok.  2 passed; 0 failed
```

rts-center 用例数 **48 → 62**（新增 `/auth/me`、账号管理、停用语义等 14 个）。

```
$ cargo clippy -p rts-center --all-targets
（本次改动零告警；仅剩 rts-core 既有的 `Configure` 未使用告警，非本次引入）
$ cargo fmt --all -- --check
（无输出，通过）
```

### 前端

```
$ bun run test
 23 pass
 0 fail
 56 expect() calls
Ran 23 tests across 1 file.

$ bun run lint      # oxlint，无输出
$ bun run typecheck # tsc -b --noEmit，无输出
```

前端用例数 **21 → 23**：删掉全部 mock 用例（前端已无 mock 实现），
换成 `fetch` 打桩的真实接口用例（登录成功、401/400/403 错误码映射、
`fetchMe` 令牌校验、no-store 验证码、网络错误兜底）。

---

## 4. 浏览器实测（AC-16）

脚本：`/tmp/rts_browser_e2e_20261031.py`（临时脚本，不入库）。
链路：`Windows Chrome headless` → `vite dev server`（同源 `/api` 代理）→ `rts-center` → 真库。
Chrome 的 CDP 只绑 `127.0.0.1`，不做任何局域网暴露。

```
[PASS] rts-user 建测试账号 :: e2e_ui_b5d8eb4f
[PASS] vite dev server 就绪
[PASS] Chrome CDP 可达
[PASS] 登录页渲染出验证码图片 :: data:image/svg+xml;base64,PHN2ZyB4bWxucz
[PASS] 从验证码图片还原出答案 :: J6JA
[PASS] 登录成功后进入首页并显示用户名 :: True
[PASS] 刷新页面后仍然是登录态（/auth/me 校验通过） :: True
[INFO] 截图已保存 /tmp/rts_webui_dashboard.png
[PASS] 找到并点击登出按钮
[PASS] 登出后回到登录页 :: True
[PASS] 登出后访问受保护页面被弹回登录页 :: True
[INFO] 已清理测试账号 e2e_ui_b5d8eb4f

全部通过
```

说明：

- 「从验证码图片还原出答案」是把服务端 SVG 里的 `<text>` 元素解出来（服务端每个字符一个
  `<text>` 节点），用于自动化测试——**服务端响应体里始终不含答案**（AC-03 已锁）。
- 「刷新页面后仍然是登录态」验证的正是新增的 `/auth/me` 校验：刷新后前端拿本地令牌
  向后端换一次用户信息，后端确认有效才保留会话。
- 截图见 `/tmp/rts_webui_dashboard.png`：右上角显示账号与角色，正文是控制台占位首页。

---

## 5. 日志检查（AC-17）

真库端到端那一轮落下的 `rts-core` 日志原文（`scripts/database/logs/app.log`，已加入 `.gitignore`）：

```
INFO rts_center: rts-center 启动中 addr=127.0.0.1:18081 … db_max_connections=5 ephemeral_jwt_secret=false
INFO rts_center::auth: 已生成图形验证码 captcha_id=9751a0a4e80a263a6dac2678e048312b expires_at=2026-10-31 11:43:03Z
INFO rts_center::auth: 登录成功 username=e2e_root_9058f9bb remember=true expires_at=2026-11-01T11:38:04Z
WARN rts_center::auth: 登录失败 username=e2e_root_9058f9bb ip=127.0.0.1
INFO rts_center::admin: 账号管理：新增账号 actor=e2e_root_9058f9bb target=e2e_292839db action="create"
INFO rts_center::auth: 登录成功 username=e2e_292839db remember=false expires_at=2026-10-31T13:38:06Z
INFO rts_center::admin: 账号管理：修改账号状态 actor=e2e_root_9058f9bb target=e2e_292839db status="disabled" action="set-status"
WARN rts_center::auth: 账号已停用，拒绝登录 username=e2e_292839db ip=127.0.0.1
INFO rts_center::admin: 账号管理：列出账号 actor=e2e_root_9058f9bb count=2
```

`grep -cE "password|口令.*=" app.log` → **0**：日志里没有任何口令字段。

各场景的日志约定：

| 场景 | 日志 |
|------|------|
| 取码 | `已生成图形验证码 captcha_id=… expires_at=…`（只记 ID 与过期时刻，不记答案） |
| 登录成功 | `登录成功 username=… remember=… expires_at=…` |
| 口令错误 | `登录失败 username=… ip=…` |
| 达到阈值 | `登录失败次数达到阈值，账号已临时锁定 username=… ip=…` |
| 停用账号 | `账号已停用，拒绝登录 username=… ip=…` |
| 管理操作 | `账号管理：新增账号 actor=… target=… action=create`（只记操作者/对象/动作，不记口令） |
| 令牌无效 | `令牌校验失败 error=…`（不记令牌本身） |

---

## 6. 已知限制

1. **AC-18 容器联调未实测**：本机没有 Docker（WSL 无 `docker`、Windows 无 Docker Desktop）。
   nginx 反代配置本次未改动，只在 Dockerfile 里删掉了 `VITE_API_MODE` 构建参数；
   同源代理路径已由 vite dev server 实测走通。**建议在有 Docker 的机器上补一次**
   `docker build` + `docker run` + 浏览器访问 `:18080` 的验证。
2. **破坏性变更**：`RTS_AUTH_USERS` 已下线。旧部署必须「建表 → `rts-user` 开超管 → 配 `DATABASE_URL`」
   三步迁移，否则服务启动即报错退出（刻意为之，不静默降级）。
3. **验证码与失败计数仍是单进程内存**：多实例部署时「取码」与「登录」必须落在同一实例；
   账号表本身是共享的。
4. **没有令牌撤销/刷新**：登出只清客户端凭证；停用账号靠 `/auth/me` 回库确认才即时生效，
   若前端不再调后端接口，则停用对已签发令牌不可见（当前唯一的自动调用点是页面挂载时的
   `/auth/me`）。
5. **管理接口没有细粒度权限**：只区分「超管」与「非超管」，角色字符串来自 `auth_user.roles`。
6. **接口级测试用内存账号表**：生产的查库路径（`PgAccountStore`）由真库端到端脚本覆盖，
   未做数据库层的自动化测试（需要真实 PG，CI 环境不具备）。
7. `sqlx` 只开 `postgres` + `runtime-tokio` + `chrono` 三个 feature，**不开 `macros`**——
   避免编译期连接数据库；代价是失去编译期 SQL 类型检查。

---

## 7. PR

| 项 | 值 |
|----|-----|
| 分支 | `feature/ui-login-real-backend` |
| PR 编号 | **#16** —— https://github.com/tao629/rts/pull/16 |
| 提交 | `6b0dd5b` feat(auth): UI 登录验证对接 rts-center，账号改由数据库承载 |
