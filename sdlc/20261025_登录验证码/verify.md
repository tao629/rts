# 验证记录（Verify）: 登录验证码

关联文档: [intent.md](./intent.md) | [spec.md](./spec.md) | [plan.md](./plan.md)
分支: `feature/login-captcha` | 日期: 2026-10-25

---

## 1. 后端验证（`server/rts-center`）

### 1.1 构建与单测

```
$ cargo build --workspace
   Compiling rts-center v0.1.0
    Finished `dev` profile [unoptimized + debuginfo] target(s)

$ cargo test -p rts-center
running 48 tests
test result: ok. 48 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 6.26s

$ cargo test --workspace
rts-center:   48 passed / 0 failed
f_matcher:    53 passed / 0 failed
r_connector:  27 passed / 0 failed
rts-core:     16 + 1 + 2 passed / 0 failed

$ cargo clippy -p rts-center --all-targets
rts-center 自身 0 warning / 0 error（仅 rts-core 原有的 Configure 未使用告警）
```

覆盖的验收点（spec §7）：

| 验收项 | 对应测试 | 结果 |
|--------|----------|------|
| A1 验证码返回三字段 + data URI 前缀 | `captcha_endpoint_returns_three_fields_with_data_uri` | ✅ |
| A2 缺失/错误验证码 → 400 INVALID_CAPTCHA | `login_with_wrong_captcha_returns_invalid_captcha`、`login_without_captcha_returns_invalid_captcha` | ✅ |
| A3 验证码一次性 | `captcha_cannot_be_used_twice` | ✅ |
| A4 验证码过期 | `expired_captcha_returns_invalid_captcha` | ✅ |
| A5 正确验证码 + 正确口令 → 200 | `login_with_correct_captcha_and_password_succeeds` | ✅ |
| A6 验证码先于口令校验 | `captcha_is_checked_before_credentials`、`wrong_password_returns_401_and_consumes_captcha` | ✅ |
| A7 失败锁定 → 423 | `account_is_locked_after_max_failures` | ✅ |
| A11 未配置账号时登录失败 | `login_fails_when_no_account_is_configured` | ✅ |
| A11 密钥未配置时随机生成 | `jwt_secret_is_random_when_not_configured` | ✅ |
| — 账号枚举防护 | `unknown_user_returns_same_error_as_wrong_password` | ✅ |
| — 账号配置解析（字段数/空字段/重名/错误信息不含口令） | users 模块 6 个用例 | ✅ |
| — 响应头 no-store | `captcha_response_has_no_store_cache_control` | ✅ |

> 凭据外置后，账号相关用例的口令改为**运行时随机生成**（源码与测试中无任何口令字面量）。

### 1.2 手工冒烟（主控独立复跑，凭据经 `RTS_AUTH_USERS` 注入）

脚本在运行时生成随机口令并注入 `RTS_AUTH_USERS`（口令不落盘、不入日志）：

```
health:  {"status":"ok","time":"2026-10-25T01:30:36Z"}

captcha: 200 id=52e8c969... code=N5TP        # 答案从 SVG <text> 节点解出

login(正确口令 + 正确验证码):        200 {"token":"eyJ0eXAiOiJKV1Qi...","tokenType":"Bearer",
                                      "expiresAt":"...","user":{"id":"u-1001","username":"admin",...}}
login(复用同一 captchaId):          400 {"code":"INVALID_CAPTCHA","message":"验证码错误或已失效，请重新输入"}
login(错口令 + 已消费的 captchaId): 400 {"code":"INVALID_CAPTCHA","message":"验证码错误或已失效，请重新输入"}
login(缺 captchaId/captchaCode):   400 {"code":"INVALID_CAPTCHA","message":"验证码缺失，请重新获取验证码"}
logout: 204
```

### 1.3 配置场景验证

| 场景 | 期望 | 实测 |
|------|------|------|
| 未配置 `RTS_AUTH_USERS` | 服务正常启动，账号表为空，登录一律失败 | 服务正常启动并 WARN；登录按校验顺序先判验证码，验证码无效时返回 `INVALID_CAPTCHA`，验证码有效时返回 `INVALID_CREDENTIALS`（见 `login_fails_when_no_account_is_configured`） |
| 未配置 `RTS_JWT_SECRET` | 启动时随机生成密钥并 WARN | 日志：`未配置 RTS_JWT_SECRET，本次使用随机密钥，令牌在进程重启后失效`；启动行 `ephemeral_jwt_secret=true` |
| `RTS_AUTH_USERS=admin:only-two-fields` | 启动报错并退出（码 1） | 退出码 1，`配置项 RTS_AUTH_USERS 无效: 第 1 个账号条目非法: 字段数应为 3 或 4（用户名:口令:显示名[:角色列表]），实际 2 个（口令不得包含 ':' 或 ';'）` |
| `RTS_AUTH_USERS=admin:pw:管理员:role;`（结尾多 `;`） | 启动报错并退出（码 1） | 退出码 1，`第 2 个账号条目非法: 条目为空，请检查 ';' 分隔符` |

> 错误信息只指明条目序号与原因，**不回显口令**（有专门用例 `parse_accounts_error_never_contains_password` 断言）。

### 1.4 日志核查

服务日志只出现 `captcha_id` / `expires_at` / `username` / `ip` / `account_count` 等字段，
**不含验证码答案、不含口令、不含令牌密钥**。

### 1.5 凭据残留扫描

```
$ grep -rnE "Admin@[0-9]+|Operator@[0-9]+|test-secret|dev-jwt-secret|DEFAULT_JWT_SECRET|BUILTIN_ACCOUNTS" server/
（无输出）
```

---

## 2. 前端验证（`web-ui`）

### 2.1 单测 / 类型 / lint

```
$ bun test
bun test v1.3.14 (1b3c11a9a)
 21 pass
 0 fail
 45 expect() calls
Ran 21 tests across 1 file. [6.56s]

$ bun run lint
$ oxlint                # 无任何告警

$ bun run build
$ tsc -b && vite build
✓ 2080 modules transformed.
dist/index.html                   0.64 kB │ gzip:  0.41 kB
dist/assets/index-BxEkg_pi.css   30.30 kB │ gzip:  6.73 kB
dist/assets/index-D0a2H0IG.js   452.64 kB │ gzip:143.84 kB
✓ built in 280ms
```

新增/调整的用例（`web-ui/tests/auth.test.ts`，共 21 个）：

| 用例 | 覆盖的验收点 |
|------|--------------|
| `fetchCaptcha` 返回非空 `captchaId` / `data:image/svg+xml;base64,` 前缀 / 未来 `expiresAt` | A9 |
| 答案 4 位且落在统一字符集内 | §2 字符集 |
| 错误验证码 → `INVALID_CAPTCHA` | A9 |
| 缺失验证码字段 → `INVALID_CAPTCHA` | A9 |
| 同一 `captchaId` 二次使用 → `INVALID_CAPTCHA`（一次性） | A3（mock 侧） |
| 校验失败同样作废验证码 | A3（mock 侧） |
| 验证码对但口令错 → `INVALID_CREDENTIALS`（验证码先于口令） | A6（mock 侧） |
| schema：空 → 「请输入验证码」；3/7 位 → 「验证码格式不正确」；4~6 位通过且 trim | §5 校验规则 |

> 测试中的演示口令改为从 mock 实现导出的 `MOCK_CREDENTIALS` 读取，测试代码内不再写死口令。

### 2.2 real 模式浏览器验证（Windows Chrome headless + CDP）

环境：后端 `rts-center` 跑在 `127.0.0.1:18099`（账号经 `RTS_AUTH_USERS` 注入），
前端 dev server 跑在 `127.0.0.1:5188`（`VITE_API_MODE=real` +
`VITE_API_TARGET=http://127.0.0.1:18099`，走 vite 代理）。

| 检查点 | 观察结果 |
|--------|----------|
| 页面渲染后端下发的验证码 | `img[alt="验证码"]` 存在，`src` 为 `data:image/svg+xml;base64,PHN2ZyB4bWxucz...`；字体/旋转特征为后端 SVG（`DejaVu Sans Mono` + `rotate`），非 mock 图；mock 演示账号块消失（`isMockMode=false` 生效） |
| 取码请求确实走后端 | CDP Network：`GET http://127.0.0.1:5188/api/v1/auth/captcha` → `RES 200`（经 vite 代理到后端） |
| 错验证码被拒 | 填 `ZZZZ` 提交 → `POST /api/v1/auth/login` → **400**；页面 Alert 显示「验证码错误或已失效，请重新输入」；`captchaCode` 与 `password` 均被清空 |
| 失败后自动刷新 | 验证码答案由 `N98Y` 变为 `2BXY`，且伴随一次新的 `GET /auth/captcha` → 200 |
| 正确验证码 + 正确口令 | 从 SVG `<text>` 提取答案填入 → `POST /auth/login` → **200** → 跳转 `/`，首页显示「登录成功 / 系统管理员 / super_admin」 |
| 前端提交字段与契约一致 | 抓到的登录请求体字段为 `username` / `password` / `remember` / `captchaId` / `captchaCode`，与 spec §3.2 逐字一致 |
| mock 模式浏览器验证 | 错码 → 同样提示「验证码错误或已失效，请重新输入」，失败后码图自动更换（`TM2V` → `UGV4`），重填正确码 + 口令 → 跳转 `/` 并显示用户信息；图片 `role="button"` / `tabindex="0"` / `aria-label="点击刷新验证码"`，聚焦后按 Enter 可刷新 |

---

## 3. 契约偏差记录（主控裁决）

| # | 偏差 | 裁决 |
|---|------|------|
| 1 | spec §2 的字符集字面值含 `L`，但同行的说明写「剔除 0/O/1/I/L」 | **以说明为准**：字符集改为 `23456789ABCDEFGHJKMNPQRSTUVWXYZ`（去掉 `L`），前后端与 spec 同步修改 |
| 2 | 锁定的触发时刻：第 N 次失败仍返回 401，下一次才返回 423 | 与 spec A7「连续 5 次失败后再试 → 423」一致，**保持实现不变** |
| 3 | `RATE_LIMITED`(429) 已定义但无触发路径 | spec §3.5 明确「与账号锁定同源，保留该码以兼容前端既有枚举」，按 spec 有意为之 |
| 4 | `ACCOUNT_DISABLED` 未实现 | spec 后端错误码表未列该码，且无「禁用账号」数据模型，暂不返回 |
| 5 | 缺 `captchaId`/`captchaCode` → `INVALID_CAPTCHA`（而非 `VALIDATION_ERROR`） | 与 spec A2 一致，**保持实现不变**；`username`/`password` 缺失或超长仍走 `VALIDATION_ERROR` |
| 6 | 登出不校验令牌、不维护黑名单 | spec §3.3「客户端本地清理凭证不依赖该接口成功」，本期最小实现，已记入 §4 已知限制 |
| 7 | 来源 IP 优先取 `X-Forwarded-For` | 无可信代理白名单时该头可伪造，仅作锁定维度的辅助信息，不作安全边界；已记入 §4 已知限制 |
| 8 | **原 spec §6 的内置账号（用户名 + 固定口令）与固定 JWT 默认密钥，违反「凭据不得入库」** | 安全审查拦截后改为**凭据一律由环境变量注入**：账号走 `RTS_AUTH_USERS`（格式见 spec §6.1，非法即启动报错退出），JWT 密钥未配置时启动随机生成。spec §6 已同步改写，并新增验收项 A11 |

---

## 4. 已知限制

| # | 限制 | 影响 | 后续 |
|---|------|------|------|
| 1 | 验证码与失败计数均为**单进程内存态** | 多实例部署时验证码不共享、失败锁定按实例计数 | 上共享存储（Redis）或改为粘性会话 |
| 2 | 登出不维护令牌黑名单 | 已签发 JWT 在有效期内仍可通过验签 | 需要时加 `jti` 黑名单或短 access token + refresh token |
| 3 | `RTS_JWT_SECRET` 未配置时用随机密钥 | 进程重启后已签发令牌失效；**多实例必须配置同一个密钥**，否则跨实例验签失败 | 部署时统一注入（已在 `server/rts-center/README.md` 标注） |
| 4 | 账号与口令由部署方注入，无用户管理能力 | 账号增删改需改环境变量并重启 | 接入真实用户存储 |
| 5 | `X-Forwarded-For` 可伪造 | 锁定维度（用户名 + IP）的 IP 部分不可信 | 引入可信代理白名单 |
| 6 | 无 CORS、无请求体大小限制、仅明文 HTTP | 前端 real 模式需同源访问或经反向代理 | 部署在反代之后并补 CORS 策略 |
| 7 | 验证码针对脚本重放，不针对 OCR | 自动化 OCR 可识别字符 | 若需对抗 OCR 需另选方案（本期明确不做） |
| 8 | mock 模式的答案以明文存在于 SVG `<text>` 中，且演示账号口令为前端内置夹具 | 仅影响演示链路，不具备真实安全强度；后端已无任何硬编码凭据 | real 模式答案由后端持有、不回传 |
| 9 | dev 下因 React StrictMode 双调用 effect，会多取一次验证码 | 多一张无用验证码被作废，不影响功能（生产构建无此现象） | 如需消除可加「已请求过」ref 守卫 |
| 10 | 失败锁定按「用户名 + IP」 | 同 IP 猜不同账号互不影响；同账号换 IP 各自独立计数（NAT 下防护削弱） | 视线上情况调整维度 |
