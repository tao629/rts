# 意图（Intent）: 让 web-ui 的登录验证真正落到 rts-center 后端（数据库账号 + 令牌校验）

Author: zhtao（仓库维护者）. 状态：finalize

## 问题（Problem）

`web-ui` 的登录验证目前**不经过任何后端校验**：

- 前端 `VITE_API_MODE` 默认取 `"mock"`，`web-ui/src/api/auth/index.ts` 据此装载前端内置的
  `MockAuthApi`：验证码在浏览器里现画、口令在浏览器里比对、令牌是 `mock-token-xxxx` 随机串，
  "登录成功"只意味着前端自己认可自己；
- 真实后端 `rts-center` 已经实现了 `GET /api/v1/auth/captcha`、`POST /api/v1/auth/login`、
  `POST /api/v1/auth/logout` 三个接口（`server/rts-center/src/routes.rs`），前端也有对应的
  `HttpAuthApi`，但**默认路径根本不会走到它**，线上部署（Docker 镜像里 `VITE_API_MODE=real`）
  与本地开发（默认 mock）行为不一致；
- `rts-center` 的账号表来自启动时的环境变量 `RTS_AUTH_USERS`（明文注入、进程内 argon2id 哈希、
  进程退出即消失）。账号不能集中管理、不能停用、不能审计，也没法给新同事开账号——
  改一个人要走"改环境变量 → 重启服务 → 所有人令牌失效"的流程；
- 令牌签发之后，前端只凭本地存储里的 `expiresAt` 判断"还登着"，没有向服务端确认过；
  令牌被服务端判为无效时，页面仍然显示为已登录。

## 预期结果（Proposed outcome）

交付一条**真正由后端裁决**的登录闭环，账号落在数据库里：

1. **前端只连真实后端**：彻底删除 `MockAuthApi`、`MOCK_CREDENTIALS`、`isMockMode` 与
   `VITE_API_MODE` 开关，登录页的验证码、校验、令牌发放全部来自 `rts-center`；
   本地 `bun run dev` 通过 vite 代理转发 `/api` 到 `rts-center`。
2. **账号改由数据库表承载**：新增 `auth_user` 表（argon2id PHC 哈希、显示名、角色、状态），
   `rts-center` 通过 `DATABASE_URL` 连接数据库，登录时按用户名查库并校验哈希；
   `RTS_AUTH_USERS` 环境变量注入账号的方式下线。
3. **新增 `GET /api/v1/auth/me`**：用 `Authorization: Bearer <token>` 换回当前用户信息。
   前端在刷新页面 / 启动时用它确认登录态，服务端判为无效（过期、签名不符、账号已停用）
   时返回 401，前端清理本地凭证并退回登录页。
4. **账号可被管理**：`rts-center` 提供账号管理接口
   （`POST/GET /api/v1/admin/users`、`PATCH .../status`、`POST .../password`），
   权限门槛为「已登录且令牌 `roles` 含 `super_admin`」；将来前端可直接接上这套接口。
   另保留一个引导 CLI（`rts-user`）只负责 bootstrap 第一个超管与应急救援——
   口令从**标准输入**读取，不进命令行历史。两条路径共用同一套 `users` 服务层，
   口令只在进程内经 argon2id 哈希后落库，仓库里不出现任何口令/哈希字面量。
5. **联调配置齐备**：vite dev 代理有默认后端地址；容器部署时 `rts-center` 监听 `0.0.0.0`、
   nginx 反代 `/api` 的说明写进文档。
6. **验证有据**：`intents/20261031_ui-login-real-backend/verify.md` 记录
   「取验证码 → 登录 → 刷新页面保持登录 → 登出」的真实实测数据（curl 闭环 + 浏览器实测），
   以及 cargo / bun 两侧测试与静态检查的实测输出。

完成标识：把前端与后端接起来后，**不启动任何 mock** 也能在浏览器里用数据库里的账号登录成功，
错误口令 / 错误验证码 / 过期令牌分别得到后端对应的错误码提示，且服务端日志能查到这次登录。

## 受影响的模块（Affected modules）

| 对象 | 改动 |
|------|------|
| `web-ui/src/api/auth/mock-auth-api.ts` | **删除**（前端 mock 登录与浏览器端验证码） |
| `web-ui/src/api/auth/http-auth-api.ts` | 改：新增 `fetchMe`；错误码映射补 `TOKEN_INVALID` |
| `web-ui/src/api/auth/types.ts` | 改：`AuthApi` 增 `fetchMe`；错误码增 `TOKEN_INVALID` |
| `web-ui/src/api/auth/index.ts` | 改：删 `VITE_API_MODE` 分支与 `MOCK_CREDENTIALS`/`isMockMode` 导出 |
| `web-ui/src/api/auth/errors.ts` | 改：错误文案表补 `TOKEN_INVALID` |
| `web-ui/src/auth/auth-provider.tsx` | 改：挂载时用 `/auth/me` 校验已存登录态，失效则清理 |
| `web-ui/src/components/login-form.tsx` | 改：删「演示账号（mock 模式）」面板 |
| `web-ui/src/vite-env.d.ts` | 改：删 `VITE_API_MODE`，补 `VITE_API_TARGET` |
| `web-ui/vite.config.ts` | 改：`/api` 代理有默认后端地址 |
| `web-ui/tests/auth.test.ts` | 改：mock 用例改为打桩 `fetch` 的真实接口用例 |
| `web-ui/Dockerfile` | 改：删 `VITE_API_MODE` 构建参数 |
| `server/rts-center/src/auth/users.rs` | 改：账号表来源由启动期环境变量改为运行期查库（`AccountStore`） |
| `server/rts-center/src/auth/db.rs` | 新建：`auth_user` 表的读写（按用户名查、插入口令哈希、改状态/口令） |
| `server/rts-center/src/auth/mod.rs` | 改：登录走 `AccountStore`；新增 `/auth/me` handler 与 Bearer 提取器 |
| `server/rts-center/src/auth/bearer.rs` | 新建：`Authorization: Bearer` 头提取器 |
| `server/rts-center/src/admin.rs` | 新建：账号管理接口（增/列/改状态/改口令）+ `super_admin` 鉴权 |
| `server/rts-center/src/routes.rs` | 改：挂载 `GET /api/v1/auth/me` 与 `/api/v1/admin/users` 系列 |
| `server/rts-center/src/error.rs` | 改：新增 `TOKEN_INVALID` / `ACCOUNT_DISABLED` / `FORBIDDEN` 错误码 |
| `server/rts-center/src/config.rs` | 改：新增 `DATABASE_URL` 等配置；下线 `RTS_AUTH_USERS` |
| `server/rts-center/src/state.rs` | 改：`AppState` 持有数据库连接池与 `AccountStore` |
| `server/rts-center/src/tests.rs` | 改：测试用内存态 `AccountStore`，不依赖真实数据库 |
| `server/rts-center/src/bin/rts-user.rs` | 新建：引导 CLI（建超管 / 应急救援），与管理接口共用服务层 |
| `server/rts-center/Cargo.toml` | 改：新增 `sqlx`（postgres + runtime-tokio）依赖 |
| `scripts/database/sql/002_auth_user.sql` | 新建：`auth_user` 建表迁移（幂等） |
| `docs/database/rts-schema.sql` | 改：并入 `auth_user` 表定义 |
| `intents/20261031_ui-login-real-backend/` | 新建：intent.md / spec.md / plan.md / verify.md |
| `server/rts-center/README.md`、`server/rts-center/CLAUDE.md`、`web-ui/README.md` | 改：更新环境变量、部署与运维说明 |

## 约束条件（Constraints）

- **凭据不入库、不入仓**：口令只从标准输入进入进程，落库的是 argon2id PHC 哈希；
  仓库、测试、日志、命令行参数里都不出现口令字面量；`DATABASE_URL` 只由环境变量注入。
- **只读主仓**：所有改动在 `.claude/worktrees/ui-login-real-backend/` 的
  `feature/ui-login-real-backend` 分支上做，主仓 `main` 不切分支、不暂存、不提交。
- **守住登录校验顺序**：验证码 → 口令 → 失败锁定，顺序与语义不变；
  账号枚举防护（账号不存在时也走一次等价 argon2 运算）不能因为改成查库而丢掉。
- **验证码一次性**：消费即作废的口令错误也作废，这条不变。
- **后端接口测试不依赖真实数据库**：`cargo test -p rts-center` 必须在没有数据库的环境里全绿，
  账号来源通过可替换的 `AccountStore` 抽象注入测试替身。
- **不引入 TLS 与 CORS 到服务本身**：对外暴露仍由反向代理（nginx）负责。
- **不做用户管理界面**：本次只提供后端管理接口与引导 CLI，不做后台管理页面。
- **不做令牌刷新 / 撤销 / 黑名单**：沿用现状（登出只清客户端凭证）。
- **DDL 走 `dba-toolkit` 技能**：建表、迁移脚本按仓库既有规范产出（备份前置、幂等、COMMENT 注解）。

## 未解决的问题（Open questions）

> 2026-10-31 提出人已全部拍板，无遗留开放项。

1. **前端 mock 的去留** → **彻底删除**（不留 `VITE_API_MODE` 开关）。
2. **是否新增 `/auth/me`** → **新增**，登录态由后端裁决。
3. **账号来源** → **改数据库用户表**（`RTS_AUTH_USERS` 下线），表名 `public.auth_user`。
4. **交付层次** → 前端对接改造 + 后端补充接口 + 开发/容器联调配置 + 文档与验证记录。
5. **账号增删改的入口** → **由 rts-center 提供管理接口**
   （`/api/v1/admin/users` 系列），将来前端可直接接；权限门槛为
   「已登录且令牌 `roles` 含 `super_admin`」，不另建权限表。
6. **第一个超管的 bootstrapping** → **保留一个引导 CLI**（`rts-user`），
   只负责建超管与应急救援，不承载业务逻辑、与管理接口共用同一套 `users` 服务层。
7. **账号读取策略** → **每次登录查库**（一次索引查询，成本远低于 argon2 校验），
   不做内存缓存与定时刷新，避免"改了库还要等 N 分钟生效"。
8. **停用账号的语义** → `status='disabled'` 的账号**登录时被拒（403 `ACCOUNT_DISABLED`）**，
   其已签发令牌在下一次调用 `/auth/me` 时**同样被判无效（401）**——停用即时生效。

## 交付后的目标形态（一句话）

> 浏览器打开登录页 → 验证码来自 `rts-center` → 口令与 `auth_user` 表里的 argon2id 哈希比对 →
> 拿到 JWT → 刷新页面时前端拿 JWT 换 `/auth/me` 确认仍然有效 → 账号可由超管通过
> `/api/v1/admin/users` 增改。链路上没有任何一个环节由浏览器自己"宣布成功"。

