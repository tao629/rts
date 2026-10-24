# 验证记录（Verify）: RTS 搬运工 Web 控制台登录页（SPA）

关联文档: [intent.md](./intent.md) · [spec.md](./spec.md) · [plan.md](./plan.md)
验证日期: 2026-10-24 | 分支: `feature/login-page-spa`

---

## 1. 自动化检查

| 命令 | 结果 |
|----|----|
| `bun install` | 通过，73 个包，无 peer 冲突 |
| `bun run lint`（oxlint） | 通过，0 error 0 warning |
| `bun run typecheck`（tsc -b） | 通过 |
| `bun run build`（tsc -b && vite build） | 通过，产物 447.6 kB / gzip 142.0 kB |
| `bun run test`（bun test） | **12 pass / 0 fail**（表单校验、mock 接口、登录态存储、跳转目标消毒） |

## 2. 浏览器端验证（Windows Chrome 155 headless + CDP 驱动）

验证脚本以 DevTools Protocol 驱动真实浏览器完成「未登录访问 → 登录 → 首页 → 退出」闭环，
共 10 项断言，**全部通过**：

| # | 验收项 | 结果 | 实测 |
|---|-------|-----|------|
| A2 | 移动端（390×844）单栏布局，无横向溢出 | ✅ | `scrollWidth - clientWidth = 0px` |
| A3 | 空表单提交显示字段级错误 | ✅ | `请输入用户名 \| 请输入密码` |
| A4 | 错误口令显示服务端错误提示 | ✅ | `用户名或密码错误` |
| A5 | 正确口令登录成功并跳转首页 | ✅ | URL → `http://localhost:5199/` |
| A5b | 登录成功轻提示 | ✅ | toast `登录成功` 出现 |
| A6 | 未登录访问 `/` 被重定向 | ✅ | `/login?from=%2F` |
| A7 | 勾选「记住我」刷新后仍保持登录 | ✅ | 刷新后仍在首页，凭证在 `localStorage` |
| A8 | 密码显示/隐藏切换 | ✅ | `type: password → text` |
| A9 | 退出登录后回到登录页且受保护页再次拦截 | ✅ | `/login?from=%2F` |
| A11 | 明暗偏好下表单区正常 | ✅ | `color-scheme: light` 固定，未出现原生控件反色 |

截图产物（开发机临时目录，未入库）：

- `login-desktop.png` —— 1440×900 双栏登录页
- `login-mobile.png` —— 390×844 移动端单栏
- `login-validation.png` —— 字段级校验态
- `login-error.png` —— 服务端错误提示态
- `dashboard.png` —— 登录后占位首页 + 成功 toast

## 3. 验证中发现并修复的缺陷

| # | 缺陷 | 根因 | 修复 |
|---|-----|-----|-----|
| D1 | 移动端登录页顶部 logo 不显示 | 页面同时渲染桌面品牌区与移动端横幅，两处 `ThemeLogo` 的 SVG 渐变 id 重复，引用串到隐藏实例 | 渐变 id 改用 `useId()` 唯一化 |
| D2 | 「登录成功」提示不出现 | 登录成功后 router state 携带的标记被 `PublicOnlyRoute` 的 `<Navigate replace>` 覆盖 | 提示信号改由 `AuthProvider` 的 `loginNotice` 持有，与路由时序解耦 |
| D3 | `tsc` 报 TS5101 | TypeScript 6.0 废弃 `baseUrl` | 删除 `baseUrl`，`paths` 单独声明 |
| D4 | oxlint `react/purity` 告警 | 渲染期调用 `Date.now()` | 提示时长改由 Provider 内部定时器管理 |

## 4. 与 plan.md 的偏差

| 计划 | 实际 | 说明 |
|-----|-----|------|
| 提示信号走 router state | 改为 Provider 状态 | 见 D2，时序更稳 |
| 计划未含测试 | 新增 `web-ui/tests/auth.test.ts` | 用 bun 内置测试覆盖校验/存储/跳转消毒 |
| `baseUrl` + `paths` 别名 | 仅 `paths` | 见 D3 |
| `login-schema.ts` 未单列 | 单列为 `src/lib/login-schema.ts` | 便于单测导入 |

## 5. 未覆盖 / 后续事项

- 未实现找回密码、注册、角色权限体系（intent.md 已明确排除）。
- 未做真实后端联调：`rts-center` 尚无认证接口，契约已定义但无服务端实现。
- 未做多语言、国际化文案（本次界面固定中文）。
- 未做首页真实监控数据可视化（本次为占位骨架）。
- 未做真实的键盘导航与读屏器（NVDA/VoiceOver）人工走查；已按 ARIA 规范编写（`role="alert"`、
  `aria-invalid`、`aria-describedby`、`aria-busy`、焦点可见、`prefers-reduced-motion` 降级）。
