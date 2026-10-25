# 构建计划（Plan）: 登录验证码

关联文档: [intent.md](./intent.md) | [spec.md](./spec.md)
分支: `feature/login-captcha`（worktree: `.claude/worktrees/login-captcha/`）

---

## 1. 改动清单

### 1.1 后端 `server/rts-center`（当前为空脚手架，需整体搭起来）

| 文件 | 动作 | 内容 |
|------|------|------|
| `Cargo.toml` | 改 | 引入 axum / tokio / serde / serde_json / jsonwebtoken / argon2 / rand / base64 等依赖；工作区依赖统一提到根 `Cargo.toml` 的 `[workspace.dependencies]` |
| `src/main.rs` | 改 | 进程入口：读配置 → 初始化日志 → 组装路由 → 启动监听 |
| `src/config.rs` | 新增 | 环境变量配置（监听地址、JWT 密钥、验证码 TTL、锁定阈值与时长） |
| `src/state.rs` | 新增 | `AppState`：`Arc<Mutex<CaptchaStore>>`、`Arc<Mutex<LoginGuard>>`、用户表 |
| `src/captcha/mod.rs` | 新增 | 验证码生成（字符集、SVG 渲染、base64 data URI）与随机源 |
| `src/captcha/store.rs` | 新增 | 内存存储：插入 / 取出（一次性消费）/ 惰性清理 / 容量上限 |
| `src/auth/mod.rs` | 新增 | 登录 / 登出处理器，编排「验证码 → 口令 → 锁定」校验顺序 |
| `src/auth/users.rs` | 新增 | 账号表：由 `RTS_AUTH_USERS` 注入，启动时 argon2 哈希后驻留内存 |
| `src/auth/token.rs` | 新增 | JWT 签发与校验（HS256） |
| `src/auth/guard.rs` | 新增 | 登录失败计数与锁定 |
| `src/error.rs` | 新增 | 统一错误类型与 `{ code, message }` 响应体、HTTP 状态映射 |
| `src/routes.rs` | 新增 | 路由装配：`/api/v1/auth/captcha`、`/api/v1/auth/login`、`/api/v1/auth/logout`、`/api/v1/health` |
| `tests/`（或模块内 `#[cfg(test)]`） | 新增 | 见 §4 验证项 |
| `README.md` | 新增 | 服务启动方式、环境变量与账号格式、接口清单与错误码（不含任何口令字面量） |

### 1.2 前端 `web-ui`

| 文件 | 动作 | 内容 |
|------|------|------|
| `src/api/auth/types.ts` | 改 | 新增 `CaptchaResponse`、`INVALID_CAPTCHA`；`LoginRequest` 加 `captchaId`/`captchaCode`；`AuthApi` 加 `fetchCaptcha()` |
| `src/api/auth/http-auth-api.ts` | 改 | 实现 `fetchCaptcha()`；`login()` 请求体带上验证码字段 |
| `src/api/auth/mock-auth-api.ts` | 改 | 客户端生成同规格 mock 验证码；`login()` 先校验验证码再校验口令 |
| `src/api/auth/errors.ts` | 改 | `INVALID_CAPTCHA` 的可读文案 |
| `src/lib/login-schema.ts` | 改 | zod schema 增加 `captchaCode`（必填、4~6 位） |
| `src/components/login-form.tsx` | 改 | 验证码图片 + 输入框；点击刷新；失败后刷新并清空；加载态 |
| `tests/auth.test.ts` | 改 | 验证码相关用例（mock 模式端到端 + 错误分支） |
| `README.md`、`.env.example` | 改 | 说明 real 模式下的验证码接口与配置 |

### 1.3 文档

- `sdlc/20261025_登录验证码/{intent,spec,plan,verify}.md`（本目录）。

---

## 2. 实施顺序与并行切分

前后端文件集完全不相交（后端只碰 `server/rts-center/**` 与根 `Cargo.toml`；前端只碰 `web-ui/**`），可并行：

```
Step 0  写 intent/spec/plan（本文件）                 —— 串行，先于开发
Step 1  ┌ 后端子任务：rts-center 验证码 + 认证 + 限流   ┐ 并行
        └ 前端子任务：web-ui 验证码接入                  ┘
Step 2  集成验证：起 rts-center，real 模式跑通端到端
Step 3  本机全量验证：cargo test / bun test / bun run build / bun run lint
Step 4  写 verify.md，提交分支，推送，开 PR
```

并行约束：
- 两侧**只以 spec.md 第 3、4 节为准**，不得单方面改动字段名；发现冲突先回到 spec 修订再改代码。
- 后端子任务不要改动 `web-ui/**`，前端子任务不要改动 `server/rts-center/**` 与根 `Cargo.toml`。
- 契约若需变更，由主控统一改 spec.md 后再同步给两侧。

---

## 3. 风险与对策

| 风险 | 影响 | 对策 |
|------|------|------|
| 前后端字段不一致导致联调失败 | 高 | 契约写在 spec.md，双方实现后由主控用 curl + 浏览器各验证一次 |
| `Cargo.toml` 依赖引入影响其他 crate | 中 | 只在根 `[workspace.dependencies]` 增项，crate 用 `workspace = true` 引用；改动后 `cargo build --workspace` 验证 |
| 新依赖在本机离线/无网络无法拉取 | 中 | 优先选已在 `Cargo.lock` 中的 crate；确需新增时先 `cargo fetch` 验证可下载 |
| SVG 验证码被 OCR 轻易识别 | 低 | 加噪点线、字符旋转/波浪扭曲；本次目标是阻断脚本暴力重放，非对抗 OCR |
| 内存验证码在多实例下不共享 | 低 | spec 已声明本次为单实例方案，部署约束写进 README |
| 口令以明文形式出现在代码/日志 | 中 | 内置账号以 argon2 哈希在启动时生成；日志不打印口令与验证码答案 |
| 前端既有 mock 演示账号提示与新增验证码冲突 | 低 | mock 模式同步生成验证码，登录页保留演示账号提示 |

---

## 4. 验证方式

### 4.1 后端

- `cargo test -p rts-center`：验证码生成/一次性消费/过期/大小写不敏感；登录成功路径；错误顺序（验证码先于口令）；锁定触发；健康检查。
- `cargo build --workspace`、`cargo clippy`（若可用）。
- 手工：`RTS_CENTER_ADDR=127.0.0.1:8080 RTS_AUTH_USERS='<用户名>:<口令>:<显示名>:<角色>' cargo run -p rts-center` 后用 curl 验证 §4.1 各验收项（A1~A7、A11）。

### 4.2 前端

- `bun test`：mock 模式下 `fetchCaptcha` 返回可用验证码；错误验证码 → `INVALID_CAPTCHA`；登录成功路径。
- `bun run build`（含 `tsc -b` 类型检查）、`bun run lint`。
- real 模式联调：`VITE_API_MODE=real bun run dev`，浏览器登录页能看到验证码图片，输入后能登录成功。

### 4.3 交付

- `verify.md` 记录实际执行的命令与结果。
- 分支 `feature/login-captcha` 提交并推送，开 PR 到 `main`。
