# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 定位

`rts-center` 是 rts 工作区（仓库根 `Cargo.toml`）中的**服务中心 crate**：搬运工平台对外的 HTTP 接口层，
当前只提供图形验证码 + 登录/登出 + 健康检查，基址 `/api/v1`。

工作区成员：`base/rts-core`（公共包，日志/配置）、`server/rts-center`（本 crate）、`toolkit/f_matcher`、
`toolkit/r_connector`。本 crate 依赖 `rts-core` 的日志模块。

契约来源：`sdlc/20261025_登录验证码/spec.md`（接口/错误码/验收点）与同目录 `intent.md`、`plan.md`、`verify.md`。
**改接口先改 spec.md**，代码只是 spec 的实现。

## 常用命令

所有 cargo 命令都在**工作区根目录**（`/home/zhtao/Code/Private/Rust/rts`）执行，本 crate 目录内执行会找不到 workspace。

```bash
cargo run -p rts-center                          # 本地启动（默认 127.0.0.1:8080）
cargo test -p rts-center                         # 48 个接口级单测
cargo test -p rts-center --lib tests::login_with_correct_captcha_and_password_succeeds -- --exact   # 跑单个用例
cargo clippy -p rts-center --all-targets         # 静态检查
cargo fmt --all                                  # 格式化
cargo build --workspace                          # 全工作区构建
```

启动需要注入凭据，否则服务照常起但所有登录失败：

```bash
export RTS_AUTH_USERS='admin:<口令>:系统管理员:super_admin'
export RTS_JWT_SECRET="$(openssl rand -hex 32)"
cargo run -p rts-center
```

`rts-core` 自身有一个已知 clippy 告警（`Configure` 未使用），不是本 crate 引入的。

## 架构

启动链路（`main.rs`）：**日志初始化 → 读配置 → 构造 `AppState` → 装配路由 → 监听 → 优雅停机**。

- 日志**先于**配置初始化：配置加载过程本身会打 WARN（未配置账号/密钥），这些告警要落进日志文件。
- `axum::serve` 用 `into_make_service_with_connect_info::<SocketAddr>()` 注入对端地址 —— 登录失败锁定
  按「用户名 + 来源 IP」计数，拿不到 IP 就退化成 `unknown`（所有同用户名的失败会共用一个计数器）。
- `_log_guard` 必须持有到进程结束，否则异步写入的日志工作线程会提前退出。

模块职责：

| 文件 | 职责 |
|------|------|
| `routes.rs` | 路由表（`/api/v1` 基址）与健康检查 |
| `auth/mod.rs` | 三个 handler + `ClientIp` 提取器 + **登录校验顺序** |
| `auth/users.rs` | `RTS_AUTH_USERS` 解析、argon2id 哈希与校验 |
| `auth/token.rs` | JWT(HS256) 签发与校验 |
| `auth/guard.rs` | 失败计数与锁定（`HashMap<用户名\0IP, 计数/锁定截止>`） |
| `captcha/mod.rs` | 自绘 SVG 验证码（不依赖图形 crate） |
| `captcha/store.rs` | 验证码内存存储：一次性消费、惰性清理、容量淘汰 |
| `config.rs` | 环境变量装载与合法性回退 |
| `error.rs` | `ApiError` → 错误码 + HTTP 状态的唯一映射处 |
| `state.rs` | `AppState`：验证码存储 / 守卫 / 账号表 / 诱饵哈希 |
| `tests.rs` | 用 `tower::ServiceExt::oneshot` 直接驱动路由的接口级测试，不占端口 |

## 必须守住的不变量

改这块代码时，下面几条是设计约束而不是实现细节，破坏它们会让接口契约失效：

1. **登录校验顺序固定**：验证码 → 口令 → 失败锁定。验证码放最前是为了让"账号不存在"与"口令错误"
   的对外表现完全一致（不泄露账号是否存在）。
2. **验证码一次性**：`CaptchaStore::consume` 取走即删，无论比对是否成功。**口令错了验证码也已作废**，
   客户端必须重新取码 —— 这条在测试 `wrong_password_returns_401_and_consumes_captcha` 里锁着。
3. **账号枚举防护**：账号不存在时也要走一次等价的 argon2 运算（`AppState::verify_dummy_password`），
   抹平耗时差异；账号不存在与口令错误返回同一个错误码 `INVALID_CREDENTIALS`。
4. **锁定阈值语义**：第 N 次失败触发上锁，但**本次仍返回 401**；从第 N+1 次起才返回 423 `ACCOUNT_LOCKED`。
   锁定维度是「用户名 + IP」，不同 IP 各自独立计数。
5. **凭据不入库**：口令、签名密钥一律由环境变量注入，仓库里不出现任何口令/密钥字面量；
   启动时对明文口令做一次 argon2id 哈希后即丢弃明文；测试里的口令也是运行时随机生成的。
   错误信息与日志**不得**包含口令或请求体原文。
6. **验证码答案不出服务端**：答案只进存储，不进响应体、不进日志；日志只记 `captcha_id` 与过期时刻。
7. **错误码唯一出口**：所有失败响应的 `code`/`status` 由 `error.rs` 的 `ApiError` 决定，不要在 handler 里手写响应体。
8. **配置容错分层**：数值型环境变量非法 → 回退默认值 + WARN；监听地址、账号配置非法 → **启动报错退出**，
   不带着半截配置跑起来。

## 已知边界（当前设计如此，不是 bug）

- **全部状态在单进程内存**：验证码、失败计数都不跨实例共享，多实例部署时"取码"与"登录"必须落在同一实例。
  要水平扩展得先换成共享存储。
- **没有令牌撤销/刷新**：登出只清客户端凭证，服务端恒返回 204，无黑名单。
- **转发头可伪造**：`X-Forwarded-For` / `X-Real-IP` 没有可信代理白名单，只作锁定维度的辅助信息，不是安全边界。
- **没有 CORS、没有 TLS**：对外暴露时由反向代理处理。
- 验证码 SVG 的干扰线/噪点只挡"脚本直读 SVG 文本"，OCR 不在对抗范围内。

## 约定

- 代码注释与文档注释用中文，公开项都带 `///` 文档注释；模块顶部用 `//!` 说明职责与"为什么这么设计"。
- 环境变量常量统一放在 `config.rs` 顶部（`ENV_*`），默认值放 `DEFAULT_*`。
- 新增接口：先在 `sdlc/<yyyyMMdd>_<slug>/spec.md` 定契约 → 实现 → 在 `tests.rs` 补接口级用例 → 记 `verify.md`。
- 本 crate 目录下有一个本地 `.env`（数据库配置），已被根 `.gitignore` 忽略，不要提交它；需要共享配置时提交 `.env.example`。
