# 构建方案（Plan）: RTS 搬运工 Web 控制台登录页（SPA）

关联文档: [intent.md](./intent.md) · [spec.md](./spec.md)
工作分支: `feature/login-page-spa`（worktree: `.claude/worktrees/login-page-spa/`）

---

## 1. 技术选型

| 关注点 | 选型 | 版本 | 理由 |
|-------|-----|-----|------|
| 框架 | Vite + React + TS | 脚手架现有 8.3.4 / 19.3.1 | 沿用现状，不折腾 |
| 包管理 | bun | 1.3.14（现有 bun.lock） | 仓库已用 bun |
| 样式 | Tailwind CSS | 4.3.3（`@tailwindcss/vite`） | v4 CSS-first 配置，零 config 文件 |
| 组件 | shadcn/ui 风格手写组件 | 基于 `@radix-ui/react-*` | copy-paste 模式，避免 CLI 交互式安装；只写本次用到的 5 个基础件 |
| 路由 | react-router-dom | 7.18.4 | 路由 + 守卫 |
| 表单 | react-hook-form + zod + @hookform/resolvers | 7.91.0 / 4.6.7 / 6.0.0 | 声明式校验，resolver 6.x 明确支持 zod v4 |
| 图标 | lucide-react | 1.61.0 | 线性图标，风格统一 |
| 类名合并 | clsx + tailwind-merge + cva | 2.1.1 / 3.7.0 / 0.7.1 | shadcn 标准三件套 |

新增依赖仅 11 个，全部为前端标配，不引入 UI 框架。

---

## 2. 目录与文件改动清单

### 2.1 新增（`web-ui/`）

```
web-ui/
├─ src/
│  ├─ lib/
│  │  ├─ cn.ts                          # clsx + tailwind-merge 封装
│  │  ├─ login-schema.ts                # 登录表单 zod 校验 schema（单测可导入）
│  │  └─ safe-redirect.ts               # 登录后跳转目标消毒（防开放重定向）
│  ├─ api/auth/
│  │  ├─ types.ts                       # LoginRequest/AuthUser/AuthErrorCode/AuthApi
│  │  ├─ errors.ts                      # AuthApiError（携带 code + httpStatus）
│  │  ├─ mock-auth-api.ts               # 内置账号 + 600ms 延迟模拟
│  │  ├─ http-auth-api.ts               # fetch 实现 + Bearer 头
│  │  └─ index.ts                       # 按 VITE_API_MODE 选择实现
│  ├─ auth/
│  │  ├─ auth-context.tsx               # 登录态存取、login/logout、useAuth
│  │  ├─ session-storage.ts             # 凭证读写 + 过期校验
│  │  └─ protected-route.tsx            # 受保护路由守卫
│  ├─ components/
│  │  ├─ ui/
│  │  │  ├─ button.tsx                  # cva 变体：default/outline/ghost/link
│  │  │  ├─ input.tsx
│  │  │  ├─ label.tsx
│  │  │  ├─ checkbox.tsx                # radix
│  │  │  └─ alert.tsx                   # 错误提示条
│  │  ├─ brand-panel.tsx                # 左侧深色品牌区（含 receive→transform→sink 动效）
│  │  ├─ password-field.tsx             # 密码框 + 显示/隐藏切换
│  │  └─ theme-logo.tsx                 # 产品标记
│  ├─ pages/
│  │  ├─ login-page.tsx                 # P1
│  │  └─ dashboard-page.tsx             # P2 占位首页
│  └─ routes/
│     └─ router.tsx                     # 路由表 + public/protected 分组
├─ tests/
│  └─ auth.test.ts                      # bun test：校验/存储/跳转消毒
```

### 2.2 修改

| 文件 | 改动 |
|----|----|
| `web-ui/package.json` | 新增依赖与脚本（`typecheck`） |
| `web-ui/vite.config.ts` | 挂 `@tailwindcss/vite` 插件；`server.proxy` 转发 `/api` → `VITE_API_TARGET`（real 模式联调用） |
| `web-ui/index.html` | `lang="zh-CN"`；标题改为「RTS 搬运工 · 控制台」 |
| `web-ui/src/index.css` | 删除脚手架样式，改写为 `@import "tailwindcss"` + `@theme` 令牌 + base 层 |
| `web-ui/src/main.tsx` | 包 `BrowserRouter` + `AuthProvider` |
| `web-ui/src/App.tsx` | 改为路由出口 `<RouterProvider>` |
| `web-ui/src/assets/hero.png`、`react.svg`、`vite.svg` | 删除（模板遗留物） |
| `web-ui/src/App.css` | 删除（模板遗留物） |
| `web-ui/public/icons.svg` | 删除（模板遗留） |
| `web-ui/README.md` | 改写为项目说明：启动、mock 账号、切换真实后端 |
| `web-ui/.env.example` | 新增：`VITE_API_MODE`、`VITE_API_BASE_URL` |

### 2.3 删除

`web-ui/src/App.css`、`web-ui/src/assets/*`、`web-ui/public/icons.svg`

---

## 3. 实施顺序

| 步 | 内容 | 产出 | 验证点 |
|----|----|----|-------|
| 1 | 依赖安装 | package.json / bun.lock 更新 | `bun install` 无报错 |
| 2 | Tailwind 接入 + 设计令牌 | index.css、vite.config.ts | 样式能编译，`cn()` 生效 |
| 3 | UI 基础件 | components/ui/*、lib/cn.ts | 类型通过，radix 渲染正常 |
| 4 | 接口契约与 mock | api/auth/* | mock 登录返回预期结构 |
| 5 | 登录态与守卫 | auth/* | 刷新后状态恢复、过期清理 |
| 6 | 页面与路由 | pages/*、routes/router.tsx、App.tsx、main.tsx | 路由跳转正确 |
| 7 | 品牌区视觉 | components/brand-panel.tsx | 动效与配色符合 spec |
| 8 | 收尾 | README、.env.example、清理模板文件 | 无模板残留 |
| 9 | 验证 | lint / build / dev 实跑 | 见第 5 节 |

> 每一步完成后跑一次 `bun run lint`，把类型与 lint 问题就地消化，避免堆积。

---

## 4. 关键实现要点

### 4.1 设计令牌（Tailwind v4 CSS-first）

在 `index.css` 中用 `@theme` 定义变量，Tailwind 自动生成工具类：

- 品牌色 `--color-brand-500: #0ea5e9` 系（青蓝），深色面板 `--color-panel-950: #0b1220` / `--color-panel-900: #111c33`。
- 语义别名 `--color-foreground` / `--color-muted-foreground` / `--color-border` / `--color-danger`。
- 圆角 `--radius-card: 12px`、输入框 `--radius-field: 8px`。
- 字体栈含 `PingFang SC` / `Microsoft YaHei`，保证中文渲染。

`html { color-scheme: light }` —— 浅色表单区固定浅色，避免系统暗色偏好下原生控件反色（对应 spec A11）。

### 4.2 登录态存储

- `localStorage` 键 `rts.auth.session`（勾选「记住我」）与 `sessionStorage` 同名键（未勾选）。
- 读取顺序 localStorage → sessionStorage；解析失败或 `expiresAt` 已过 → 清除并视为未登录。
- `remember` 变化时，先清掉另一处存储，避免双份凭证。

### 4.3 守卫

`ProtectedRoute` 在渲染前读取登录态：

- 无效 → `<Navigate to={`/login?from=${encodeURIComponent(location.pathname + location.search)}`} replace />`
- 有效 → 渲染子路由

`/login` 用 `<PublicOnlyRoute>`：已登录 → `<Navigate to="/" replace />`。

### 4.4 `from` 安全

登录成功后跳 `from`，但必须校验：以 `/` 开头且不以 `//` 开头（防开放重定向），否则回退到 `/`。

### 4.5 mock 行为

- 账号：`admin / Admin@123`（super_admin）、`operator / Operator@123`（operator）。
- 延迟 600ms 模拟网络；口令错误或账号不存在 → `INVALID_CREDENTIALS`。
- token 为可辨识的假串（`mock-token-<随机>`），`expiresAt` = now + 24h（勾选记住）或 now + 2h（未勾选）。

---

## 5. 验证方式

### 5.1 自动化

```bash
cd web-ui
bun install
bun run lint        # oxlint
bun run build       # tsc -b && vite build
bun run typecheck   # tsc -b --noEmit
```

### 5.2 运行时（对应 spec 第 7 节 A1–A11）

1. `bun run dev` 启动，确认 HTTP 200 且 SPA fallback 生效（`/login` 直连可访问）。
2. 浏览器手测清单：A1 双栏、A2 375px 单栏、A3 空提交校验、A4 错误口令、A5 正确登录跳转、A6 未登录重定向带 `from`、A7 记住我刷新保持、A8 密码显隐、A9 登出后再拦截、A11 系统暗色偏好下表单区正常。
3. mock/real 切换验证：设 `VITE_API_MODE=real` 时应走 fetch（表现为请求失败 → `NETWORK_ERROR` 提示），确认切换开关有效。

### 5.3 交付验证

- 主仓 `main` 分支保持干净（不切分支、不提交）。
- worktree 分支提交后推送并创建 PR，PR 描述附 spec 验收对照表。

---

## 6. 风险与对策

| # | 风险 | 影响 | 对策 |
|---|-----|------|------|
| R1 | Tailwind v4 与 v3 配置方式不同（无 `tailwind.config.js`，用 `@theme`） | 样式不生效 | 严格按 v4 CSS-first 写法；build 后检查产物里有对应工具类 |
| R2 | zod v4 与 `@hookform/resolvers` v6 的 API 差异 | 类型/运行时错误 | 已核对 resolver 6.0.0 支持 `zod ^4.0.0`；只用基础 `z.string()/min/max`，避开 v4 破坏性 API |
| R3 | Radix 组件与 React 19 的 peer 依赖告警 | 安装告警 | 选用支持 React 19 的最新版 radix；install 后确认无 peer 冲突 |
| R4 | `bun install` 改动 `bun.lock` 与既有 node_modules 版本冲突 | 构建失败 | 安装后立即跑 `bun run build` 验证；必要时用 `bun add` 精确锁版本 |
| R5 | 无 `gh` CLI，无法命令行创建 PR | 交付受阻 | 先推送分支；PR 通过 GitHub compare 链接人工创建，或用 REST API（需 token） |
| R6 | 主仓 `web-ui/` 是未跟踪的脚手架副本，与 worktree 内版本分叉 | 本地两个副本不一致 | 本次以 worktree 版本为准；交付后由发起人决定是否删除主仓那份未跟踪副本 |
| R7 | 深色品牌区动效（流动光点）性能与 `prefers-reduced-motion` | 无障碍/低端机卡顿 | 仅用 CSS `transform/opacity` 动画；在 `prefers-reduced-motion: reduce` 下降级为静态 |
| R8 | 中文字体在不同系统渲染差异 | 排版错位 | 字体栈含 `PingFang SC` / `Microsoft YaHei` / `Noto Sans CJK SC` 兜底 |

---

## 7. 回滚方案

所有改动集中在 `feature/login-page-spa` 分支的 `web-ui/` 新增目录与 `sdlc/20261024_登录页SPA设计/` 文档，不触碰既有 Rust 代码。回滚 = 关闭分支并删除 worktree（`git worktree remove .claude/worktrees/login-page-spa`），主仓不受影响。
