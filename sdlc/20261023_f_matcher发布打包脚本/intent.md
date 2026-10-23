# 意图（Intent）: f_matcher 发布打包脚本固化到仓库

Author: zhtao. 状态：Finalize |

## 问题（Problem）

`toolkit/f_matcher` 的发布产物目前只能靠手工命令拼出来，且存在三个反复踩坑的点：

1. **release 优化参数靠临时环境变量注入**：出包时要手工设置 `CARGO_PROFILE_RELEASE_LTO`、`CARGO_PROFILE_RELEASE_CODEGEN_UNITS`、`CARGO_PROFILE_RELEASE_STRIP` 三个变量，漏设一个就得到一个未 strip / 未开 LTO 的产物，而且没有第二个人会知道该设哪几个。
2. **产物不可直接分发**：编译结果散落在 `target/` 下，是一堆中间产物与依赖文件；`sensitive_rule.txt` 不会跟着二进制走——只把二进制拷到目标机后，工具的默认规则查找（exe 同级目录）会失败，必须额外记得带一个 `--rule` 参数。
3. **没有目标平台约定**：本机默认编译出的是 glibc 动态链接产物，拷到 glibc 版本较老的服务器上会报 `GLIBC_2.xx not found`；到底该出哪个架构、哪种链接方式，全凭临场决定。

需要一个固化到仓库的打包脚本：**一条命令产出可直接拷到目标机的发布包**——包含静态链接二进制、与二进制同级的规则文件、使用说明、校验和文件。

## 预期结果（Proposed outcome）

**可度量的完成标识：**

1. 仓库新增 `scripts/build_release.sh`，一条命令完成：编译 → 组装产物目录 → 打包 tar.gz → 生成 sha256 校验文件。
2. 产物布局（`dist/` 下，**不入版本库**）：

   ```text
   dist/
   ├── f_matcher-0.1.0-linux-x86_64/
   │   ├── f_matcher              # musl 静态二进制
   │   ├── sensitive_rule.txt     # 规则文件，与二进制同级
   │   └── README.md              # 用法说明
   ├── f_matcher-0.1.0-linux-x86_64.tar.gz
   └── f_matcher-0.1.0-linux-x86_64.tar.gz.sha256
   ```

   版本号取自 `toolkit/f_matcher/Cargo.toml` 的 `version`，不硬编码。
3. **release 优化配置写入工作区 `Cargo.toml`**（`[profile.release]`：`lto` / `codegen-units = 1` / `strip`），使 `cargo build --release` 与发布产物天然一致，脚本不再需要注入环境变量。
4. **支持两个目标平台**，均为 musl 静态链接（目标机无需安装 glibc 等运行库）：
   - `x86_64-unknown-linux-musl`（本机原生）
   - `aarch64-unknown-linux-musl`（交叉编译，用 `rust-lld` 自包含链接，**不需要安装交叉工具链**）
   默认只出本机架构；`--all` 出两个平台。
5. 脚本行为可预期、可自解释：
   - `--help` 打印用法与支持的平台。
   - 目标平台未安装 `rust-std` 时，给出明确的安装提示而不是晦涩的链接错误。
   - 打包完成后打印产物路径、大小与 sha256 摘要。
6. `dist/` 加入 `.gitignore`，构建产物不污染工作树（`git status` 保持干净）。
7. 打包出的二进制经过验证：静态链接（`ldd` 显示 `statically linked`）、可在与仓库无关的目录下独立运行（默认规则查找命中 exe 同级目录）、对 UTF-8 与 GBK 文件均能正确匹配。

**验证方式（本地可自测）：**

- 脚本级：`bash scripts/build_release.sh --help`；`bash scripts/build_release.sh`（出本机平台）；`bash scripts/build_release.sh --all`（出两个平台）；产物结构、sha256 可校验。
- 产物级：解包到临时目录，从其它 cwd 运行打包内的二进制，扫描 UTF-8 + GBK 混合测试目录，核对输出的路径:行号与退出码。
- 回归：`cargo build --workspace` 与 `cargo test -p f_matcher` 仍通过（本次不改动 f_matcher 业务逻辑）。

## 受影响的模块（Affected modules）

| 模块 | 路径 | 变更性质 |
|----|----|----|
| 发布打包脚本 | `scripts/build_release.sh` | 新增：编译 + 组装 + 打包 + 校验和 |
| 工作区清单 | `Cargo.toml` | 改：新增 `[profile.release]` 段 |
| 忽略规则 | `.gitignore` | 改：新增 `/dist/` |
| 敏感文件匹配工具 | `toolkit/f_matcher` | **不改动业务逻辑**，仅被脚本读取版本号、规则文件与 README 用于组装产物 |

不涉及数据库、不涉及 rts-center、不改动 `rts-core` 与 `r_connector` 的代码。

## 约束条件（Constraints）

**技术约束**

- 遵循仓库既有规约：走 `立项 → 开发 → 验证 → 交付 → 发布` 流程，产出物落在 `sdlc/20261023_f_matcher发布打包脚本/`；所有开发在独立 worktree（`.claude/worktrees/f-release/`）中进行，本机验证通过后提交分支并提 PR；主仓只用于拉取与只读查看。
- 脚本只用 bash + cargo + tar + sha256sum，不引入新的第三方构建工具；无网络依赖（依赖已进 `Cargo.lock`，构建可离线进行）。
- 交叉编译 aarch64 时用 `rust-lld` + `link-self-contained`，避免要求开发者安装交叉工具链。
- 脚本必须可在任意工作目录调用（自行定位仓库根），且失败时以非零退出码结束并给出可诊断的提示。

**明确不做（本次范围外）**

- 不做 Windows / macOS 产物（只出 musl 静态 Linux 产物）。
- 不做安装包（deb/rpm）、不做容器镜像、不做自动上传到制品库/发布到 GitHub Releases。
- 不做代码签名与 SBOM 生成。
- 不改动 f_matcher 的任何业务逻辑与行为。

## 已解决的问题（Resolved questions）

1. **支持的目标平台**（已确认）：`x86_64-unknown-linux-musl` 与 `aarch64-unknown-linux-musl` 两个，默认出本机架构，`--all` 出两个。
2. **release 优化配置的位置**（已确认）：写入工作区 `Cargo.toml` 的 `[profile.release]`，不用脚本内环境变量注入。
3. **产物形式**（已确认）：目录 + 同名 `.tar.gz` + `.tar.gz.sha256` 校验文件；`dist/` 加入 `.gitignore`。
4. **是否在打包后自动冒烟测试**（已确认）：本次**不做**自动冒烟测试（选项中未选），改为在验证环节由人工/单测覆盖；脚本只负责构建与打包。

## 仍需在 spec/plan 阶段细化的事项

- 脚本参数的具体形态（`--target` / `--all` / `--output-dir` / `--help`）与错误提示文案。
- `[profile.release]` 各字段的取值（lto 模式、`opt-level` 是否调整、`panic` 是否 abort）。
- 产物目录命名规则（版本 + 平台 triple 的组合方式）与 sha256 校验文件的写法（`sha256sum -c` 可直接校验）。
