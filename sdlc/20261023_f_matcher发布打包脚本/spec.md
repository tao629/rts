# 需求规格说明书（Spec）: f_matcher 发布打包脚本固化到仓库

依据：`intent.md`（Finalize）。状态：Draft，待发起人评审 |

## 1. 范围

在仓库中固化 f_matcher 的发布打包流程：新增 `scripts/build_release.sh`（一条命令完成编译 → 组装 → 打包 → 校验和），把 release 优化配置写入工作区 `Cargo.toml` 的 `[profile.release]`，并把产物目录 `dist/` 加入 `.gitignore`。

不在本次范围：Windows/macOS 产物、deb/rpm 安装包、容器镜像、制品库上传、GitHub Release 自动发布、代码签名与 SBOM、f_matcher 业务逻辑改动。

## 2. 命令行接口

```text
scripts/build_release.sh [选项]

选项：
  --target <triple>   指定目标平台，默认按本机架构选择对应的 musl triple
  --all               为全部支持的平台各出一份产物
  --output-dir <dir>  产物输出目录，默认 <仓库根>/dist
  --help              打印用法后退出 0
```

| 情形 | 行为 | 退出码 |
|------|------|--------|
| 正常打包 | 打印产物路径、大小、sha256 | 0 |
| `--help` | 打印用法与支持的平台列表 | 0 |
| 未知选项 / 缺少选项取值 | stderr 打印用法 | 1 |
| `--target` 指定了不支持的平台 | stderr 列出支持的平台 | 1 |
| 本机架构不在支持列表且未指定 `--target` | stderr 提示用 `--target` 指定 | 1 |
| 目标平台的 `rust-std` 未安装 | stderr 给出 `rustup target add <triple>` 提示 | 1 |
| 编译 / 打包失败 | 打印错误并透传失败 | 非零 |

## 3. 支持的目标平台

| triple | 产物平台标识 | 说明 |
|--------|--------------|------|
| `x86_64-unknown-linux-musl` | `linux-x86_64` | 本机原生构建（x86_64 主机） |
| `aarch64-unknown-linux-musl` | `linux-aarch64` | 交叉编译，用 `rust-lld` 自包含链接，**无需交叉工具链** |

两者均为 **musl 静态链接**，目标机不需要安装 glibc 等运行库。

## 4. 产物布局

产物输出到 `<output-dir>/`（默认 `<仓库根>/dist`）：

```text
dist/
├── f_matcher-0.1.0-linux-x86_64/
│   ├── f_matcher                 # 二进制，权限 0755
│   ├── sensitive_rule.txt        # 规则文件（与二进制同级，默认规则查找才能生效）
│   └── README.md                 # 用法说明
├── f_matcher-0.1.0-linux-x86_64.tar.gz
└── f_matcher-0.1.0-linux-x86_64.tar.gz.sha256
```

命名规则：`f_matcher-<version>-<平台标识>`。

- `<version>`：取自 `toolkit/f_matcher/Cargo.toml` 的 `version` 字段（脚本解析，不硬编码）。
- `<平台标识>`：`linux-<架构>`，架构取自 triple 的第一段（`x86_64` / `aarch64`）。
- tar.gz 内含一层顶层目录 `f_matcher-<version>-<平台标识>/`，解压后即得完整包。
- `.sha256` 内容为 `sha256sum` 标准格式（`<hash>  <文件名>`），可直接用 `sha256sum -c` 校验。

## 5. 脚本行为规格

1. **定位仓库根**：以脚本自身路径（`scripts/` 的上级）为准，与调用时的当前工作目录无关；所有路径基于仓库根解析。
2. **读取版本号**：从 `toolkit/f_matcher/Cargo.toml` 解析 `version = "x.y.z"`；解析不到则报错退出 1。
3. **目标平台选择**：
   - `--target <triple>`：只打指定平台；不在支持列表 → 报错退出 1。
   - `--all`：按支持列表的顺序逐个平台打包。
   - 默认：由 `uname -m` 映射（`x86_64` → x86_64 musl；`aarch64` → aarch64 musl）；其它架构报错并提示用 `--target` 指定。
4. **目标平台检查**：用 `rustup target list --installed` 确认该 triple 的 `rust-std` 已安装；未安装则报错并提示 `rustup target add <triple>`（不自动安装，避免脚本隐式改动工具链状态）。
5. **编译**：
   - 命令：`cargo build --release -p f_matcher --target <triple>`。
   - 交叉编译（target 与本机架构不一致）时设置 `CARGO_TARGET_<TRIPLE 大写下划线>_LINKER=rust-lld` 与 `RUSTFLAGS` 追加 `-C link-self-contained=yes`，使 musl 自包含链接生效、不依赖交叉工具链。
   - 若环境已有 `RUSTFLAGS`，追加而非覆盖。
6. **组装**：把 `target/<triple>/release/f_matcher`、`toolkit/f_matcher/sensitive_rule.txt`、`toolkit/f_matcher/README.md` 复制到产物目录；二进制置 0755。
   - 打包前先删除同名旧产物目录与同名旧 tar.gz / sha256，保证产物是干净重建的。
7. **打包与校验和**：`tar -czf` 生成 tar.gz；`sha256sum` 生成 `.sha256`（内容中的文件名为 tar.gz 的 basename，便于 `cd dist && sha256sum -c` 校验）。
8. **输出摘要**：每个平台打印产物目录、tar.gz 路径、文件大小、sha256 摘要。
9. **健壮性**：`set -euo pipefail`；任何步骤失败立即以非零码退出并打印中文错误提示；不留下半成品目录（失败时清理临时产物目录）。

## 6. release profile

工作区 `Cargo.toml` 新增：

```toml
[profile.release]
lto = true
codegen-units = 1
strip = true
```

- `lto = true`：跨 crate 链接期优化。
- `codegen-units = 1`：单编译单元，利于优化。
- `strip = true`：去除符号表，减小分发体积。
- `opt-level` 保持默认（3）：本工具以批量扫描大文件为主，优先吞吐速度。
- `panic` 保持默认（unwind）：不改变现有行为。

该 profile 对工作区所有 crate 生效（`r_connector` 等同样受益），并使手工 `cargo build --release` 与发布产物一致。

## 7. 忽略规则

`.gitignore` 新增 `/dist/`：构建产物不进入版本库，工作树保持干净。

## 8. 其它约束与澄清

| 编号 | 事项 | 处置 |
|----|----|----|
| C1 | 交叉编译 aarch64 的链接器 | 用 `rust-lld` + `link-self-contained=yes`，避免要求开发者安装 `aarch64-linux-gnu-gcc` 等交叉工具链（已实测可行） |
| C2 | 脚本是否自动安装缺失的 rust-std | 不自动安装（会隐式改动工具链状态），改为报错并给出安装命令 |
| C3 | 产物是否入版本库 | 不入库，`.gitignore` 加 `/dist/` |
| C4 | 打包后是否自动冒烟测试 | 本次不做（脚本只负责构建与打包），验证由本阶段人工验证与既有单测覆盖 |
| C5 | 规则文件如何随包分发 | 放在产物目录中与二进制同级，保证工具的默认规则查找（exe 同级目录）生效 |

## 9. 验证方式

1. `bash scripts/build_release.sh --help`：打印用法与平台列表，退出码 0。
2. `bash scripts/build_release.sh`：出本机平台（linux-x86_64）产物；目录结构、文件权限、tar.gz 内容、sha256 均符合 §4；`cd dist && sha256sum -c *.sha256` 通过。
3. `bash scripts/build_release.sh --all`：同时出 linux-x86_64 与 linux-aarch64 两份产物；`file` 显示 aarch64 产物为 ARM aarch64 静态可执行文件。
4. 产物独立可用性：解包到临时目录，**从其它 cwd** 运行包内二进制，扫描 UTF-8 + GBK 混合测试目录，核对输出的 `路径:行号` 与退出码（3）。
5. 回归：`cargo build --workspace` 通过；`cargo test -p f_matcher` 37 个用例仍全部通过。
6. `git status --porcelain` 为空（`dist/` 被正确忽略）。

## 10. 不做（重申）

- 不出 Windows/macOS 产物。
- 不做安装包、容器镜像、制品库上传、GitHub Release 自动发布。
- 不做代码签名与 SBOM。
- 不改动 f_matcher 业务逻辑。
