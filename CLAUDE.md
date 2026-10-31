# CLAUDE.md

## 项目概览
这是一个搬运工项目，专注于任务(receive->transform->sink)调度、监控。


## 模块结构
|模块|路径|定位|
|----|----|----|
|公共包|base/rts-core|平台封装的公用功能包，如日志、配置等|
|服务中心|server/rts-center|收揽平台资源访问接口，通过 REST 服务对其它模块开放|
|远程连接工具|toolkit/r_connector|FTP/SFTP连接远程，执行用户输入指令|
|敏感关键字扫描工具|toolkit/f_matcher|递归扫描目录下的 .txt/.csv，按规则文件匹配关键字，输出 `路径:行号`|
|Web 控制台|web-ui|Vite + React + TS 的前端 SPA（登录页、路由守卫、流水线示意图），bun 管理依赖与构建；已容器化（Dockerfile + nginx）|

cargo workspace 成员：`base/rts-core`、`server/rts-center`、`toolkit/f_matcher`、`toolkit/r_connector`；
`web-ui` 不在 cargo workspace 内，是独立的 bun 工程。各模块的详细约定见各自目录下的
`README.md` / `CLAUDE.md`（如 `server/rts-center/`）。


## 常用命令
cargo 命令在工作区根目录执行：

```bash
cargo build --workspace
cargo test --workspace
cargo test -p rts-center                              # 单 crate
cargo test -p rts-center --lib tests::login_with_correct_captcha_and_password_succeeds -- --exact  # 单个用例
cargo clippy -p <crate> --all-targets
cargo fmt --all
```

前端命令在 `web-ui/` 目录执行：

```bash
bun install
bun run dev        # 开发服务器
bun run build      # 类型检查 + 生产构建
bun run lint       # oxlint
bun run typecheck  # tsc -b --noEmit
bun run test       # bun test
```


## 开发流程：立项 → 开发 → 验证 → 交付 → 发布
新需求的立项产物统一放 `intents/<yyyyMMdd>_<slug>/` 目录（`intents/` 下是历史遗留目录，保持原样不迁移）；
从 `立项` 环节开始，开发环节在独立 `worktree` 中进行，本地验证通过后进行分支提交并创建PR

### 立项
使用 `intent-driven` 技能处理，依次输出 `intent.md -> spec.md -> plan.md`

```text
intents/20261015_华为核心网审计日志/
├── intent.md      # 规划阶段(Plan)输出物：需求意图梳理结果
├── spec.md        # 设计阶段(Design)输出物：依据 intent.md 生成的需求规格说明书 
└── plan.md        # 构建阶段(Build)输出物：依据 spec.md 需求的可行性方案, 改哪些文件、顺序、风险、验证方式
```

`intent.md` 未确认前不进入下一步；`spec.md` 是接口/行为的唯一契约来源，**改契约先改 spec.md**，
代码只是它的实现。`spec.md` 定稿后再写 `plan.md`（改动清单、顺序、风险、验证方式）。

### 开发
`plan.md`确认后进入开发环节，未确认不开发.
* 在独立的 `worktree (.claude/worktrees/<slug>/)` 下进行，禁止在主仓开发，主仓只用于拉取和只读查看，不切分支、不暂存、不提交
* 数据库操作优先执行, 使用 `scripts/database` 脚本 

分支命名 `feature/<slug>`（先例：`feature/login-captcha`、`feature/login-page-spa`、
`feature/captcha-width`、`feature/f-matcher-perf`、`feature/webui-docker`）。

### 验证
按改动范围跑，**命令与实测结果都要留下记录**：

```bash
# 后端 / 工具链
cargo test --workspace
cargo clippy -p <crate> --all-targets
cargo fmt --all -- --check

# 前端（web-ui/）
bun run test && bun run lint && bun run typecheck
```

- 手工冒烟：起 `rts-center`（凭据经环境变量注入），用 curl 走通「取验证码 → 登录 → 登出」闭环；
  前端改动用 **Windows Chrome headless + CDP** 驱动验证（WSL 里没有浏览器）。
- 验证记录写入 `intents/<slug>/verify.md`：逐条对照 `spec.md` 的验收标准、贴实测数据、
  写明已知限制与结论；交付前把 PR 编号回填进去。

### 交付
验证通过后推分支，建PR，报PR编号[number]。

- 提交信息用中文 + conventional 前缀：`feat(web-ui): 新增 SPA 登录页与路由守卫`、`fix(auth): ...`、`docs(sdlc): ...`。
- 本机**没有 gh CLI**，建 PR 走 GitHub REST API：token 从 `~/.git-credentials` 取，
  用 Python `urllib` 直接调 `https://api.github.com/repos/tao629/rts/pulls`。
- PR 描述末尾附 `🤖 Generated with [Claude Code](https://claude.com/claude-code)`。

#### 合入 main 与删分支：先确认，再动远端
本仓**没有配置 CI**，PR 上通常也没有 review 记录，因此「合入 main」「删除远端分支」这类
**远端写操作会被 Claude Code 的权限拦截判为"未经评审就合并"**，需要向用户做一次**显式确认**
才会放行——对话里一句"合并 PR #N"不足以满足它，别反复重试。

可行流程：

1. 本地合并：`git merge --no-ff origin/<branch>`，提交信息写成
   `Merge pull request #N from <owner>/<branch>`（GitHub 据此自动把 PR 标记为 merged）；
2. 推送前跑完相关测试；
3. **向用户明确确认**后再 `git push origin main`；
4. 删除分支同样先确认范围（只删本地 / 只删远端 / 都删）。两个分支的提交都已在 main 上时，
   `git branch -d`（安全模式，未合入会拒绝）即可删本地，`git push origin --delete <branch>` 删远端。

本地 `main` 保持只读：不切分支、不暂存、不提交，改动一律在 worktree 里的分支上做。

### 发布
两条发布路径：

- **`toolkit/f_matcher`（二进制包）**：`scripts/build_release.sh` 编译 musl 静态二进制
  （`x86_64-unknown-linux-musl` / `aarch64-unknown-linux-musl`），组装产物目录、打 tar.gz 并生成
  sha256 校验文件，产物落在 `dist/`（已 gitignore）。跨平台目标需先 `rustup target add <triple>`。

  ```bash
  scripts/build_release.sh            # 本机架构
  scripts/build_release.sh --all      # 全部支持平台
  ```

- **`web-ui`（容器镜像）**：`web-ui/Dockerfile` 多阶段构建（bun 构建 → nginx 托管），**非 root 运行**、
  容器内监听 **8080**；运行时用 `API_UPSTREAM` 指定后端地址（entrypoint 用 `envsubst '$API_UPSTREAM'`
  定向渲染 nginx 配置，避免误替换 `$uri` 之类的 nginx 变量）。构建与运行参数见
  `web-ui/README.md` 的「容器化」章节。
  注意：宿主 `-p` 端口**不能与后端同端口**，否则端口转发会静默失效（容器起得来但 `docker port` 查不到映射）。


## 全局约定
- **凭据不入库**：口令、签名密钥一律由环境变量注入，仓库里不出现任何口令/密钥字面量；
  测试里的口令也改为运行时随机生成。日志与错误信息里不得出现口令、请求体原文、验证码答案。
- **注释与文档用中文**，公开项带 `///` 文档注释，模块顶部用 `//!` 说明职责与设计取舍。
- `.env` 已被 `.gitignore` 忽略；需要示例配置时提交 `.env.example`。
