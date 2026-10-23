# 构建方案（Plan）: f_matcher 发布打包脚本固化到仓库

依据：`spec.md`。状态：Draft |

## 1. 改动文件清单与顺序

| 序 | 文件 | 变更 | 说明 |
|----|----|----|----|
| 1 | `Cargo.toml` | 改 | 新增 `[profile.release]`：`lto = true` / `codegen-units = 1` / `strip = true` |
| 2 | `.gitignore` | 改 | 新增 `/dist/` |
| 3 | `scripts/build_release.sh` | 新增 | 编译 → 组装 → 打包 → sha256 的完整打包脚本（可执行位） |
| 4 | `sdlc/20261023_f_matcher发布打包脚本/spec.md` | 改 | 按实现结果回填偏差 |
| 5 | `sdlc/20261023_f_matcher发布打包脚本/plan.md` | 改 | 追加"实现结果与偏差记录"与"验证结果" |

依赖方向：3 依赖 1（profile 决定产物形态）与 2（产物目录约定）；1、2 相互独立。故 1→2→3 顺序开发，每步保持 `cargo build --workspace` 通过。

## 2. 关键技术点（已核实的事实）

- **musl 静态链接**：`cargo build --release --target x86_64-unknown-linux-musl` 产出 `static-pie linked` 的自包含 ELF（实测 631KB，strip 后）。
- **aarch64 交叉编译**：直接用宿主 `cc` 交叉链接会失败（`collect2: ld returned 1`）；设置
  `CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld` + `RUSTFLAGS="-C link-self-contained=yes"`
  后**实测成功**，产出 `ARM aarch64, statically linked, stripped`。因此脚本不需要任何交叉工具链。
- **`rust-lld` 随 rustup 工具链提供**（本机 Rust 1.98.1 可直接调用），无需额外安装。
- **triple → 环境变量名**：Cargo 约定 `<triple>` 大写并将 `-` 换 `_` 前缀 `CARGO_TARGET_`，如
  `CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER`。
- **`rustup target list --installed`** 可用于判断某 triple 的 `rust-std` 是否就绪。
- **版本号解析**：`toolkit/f_matcher/Cargo.toml` 中 `version = "0.1.0"`，用 `sed -n 's/^version *= *"\(.*\)"/\1/p'` 取首个匹配即可（Cargo.toml 中 version 只出现在 `[package]` 段）。
- **sha256 校验文件**：`sha256sum <file>` 输出 `<hash>  <basename>`，在 `dist/` 目录下执行 `sha256sum -c <file>.sha256` 即可校验。

## 3. 实现要点（scripts/build_release.sh）

1. **定位仓库根**：`REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"`，随后 `cd "$REPO_ROOT"`；脚本可在任意 cwd 调用。
2. **参数解析**：`--target` / `--all` / `--output-dir` / `--help`；未知选项或缺值 → 打印用法到 stderr 并 `exit 1`。
3. **平台表**：
   ```bash
   SUPPORTED_TARGETS=(x86_64-unknown-linux-musl aarch64-unknown-linux-musl)
   ```
   默认按 `uname -m` 映射；`--target` 校验在表内；`--all` 遍历全表。
4. **目标检查**：`rustup target list --installed | grep -qx "$triple"`，未装则提示 `rustup target add $triple` 后 `exit 1`。
5. **编译**：交叉编译时导出 `CARGO_TARGET_<TRIPLE>_LINKER=rust-lld` 与 `RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }-C link-self-contained=yes"`（追加而非覆盖）。
6. **组装**：
   - 包名 `f_matcher-<version>-linux-<arch>`，`<arch>` 取 triple 首段。
   - 先 `rm -rf` 同名旧目录与同名旧 tar.gz/sha256，避免半成品与陈旧文件混入。
   - 拷贝二进制、`toolkit/f_matcher/sensitive_rule.txt`、`toolkit/f_matcher/README.md`；`chmod 0755` 二进制。
7. **打包与校验**：`tar -czf` → `sha256sum`（在 output-dir 内执行以得到裸文件名）→ `mv` 出校验文件。
8. **摘要输出**：平台、产物目录、tar.gz 路径、大小（`du -h`）、sha256 摘要。
9. **失败清理**：编译/打包失败时删除半成品产物目录再退出。

## 4. 风险与对策

| 风险 | 影响 | 对策 |
|----|----|----|
| `rust-std` 未安装 | 构建失败且提示晦涩 | 脚本预检查并给出 `rustup target add <triple>` 的明确提示 |
| aarch64 交叉链接失败 | 出不了 arm64 产物 | 用 `rust-lld` + `link-self-contained`（已实测）；脚本对交叉目标一律走该路径 |
| 用户环境已有 `RUSTFLAGS` | 被覆盖导致行为变化 | 追加而非覆盖 |
| 已有 `RUSTFLAGS` 与 `-C link-self-contained` 冲突 | 链接失败 | 追加项幂等，重复传入无副作用 |
| 产物目录积压旧版本 | 混淆分发对象 | 打包前清理同名旧产物；不同版本目录并存不影响使用 |
| `dist/` 误入版本库 | 仓库膨胀 | `.gitignore` 加 `/dist/`，并在验证时用 `git status` 确认 |
| `[profile.release]` 影响其它 crate | r_connector 产物变小变快 | 均为正向影响；不改变行为（未改 panic/opt-level） |
| 脚本依赖 `sha256sum`/`tar` | 精简容器里缺失 | 均为 coreutils/标准工具，构建环境本就依赖 cargo；缺失时脚本报错退出 |

## 5. 验证方式

1. `cargo build --workspace`（加 profile 后仍通过）。
2. `bash scripts/build_release.sh --help` → 用法与平台列表，退出码 0。
3. `bash scripts/build_release.sh` → 出 linux-x86_64 产物；核对目录结构、`file` 显示 static-pie、`sha256sum -c` 通过。
4. `bash scripts/build_release.sh --all` → 追加 linux-aarch64 产物；`file` 显示 ARM aarch64 静态可执行。
5. 产物独立可用性：解包到 `/tmp`，从其它 cwd 运行，扫描 UTF-8 + GBK 混合目录，核对 `路径:行号` 与退出码 3。
6. `cargo test -p f_matcher` → 37 个用例仍全部通过。
7. `git status --porcelain` 为空。

## 6. 实现结果与偏差记录

| 序 | 事项 | 与方案/spec 的偏差 | 原因 |
|----|----|----|----|
| 1 | 产物组装方式 | 由"直接在产物目录组装 + 失败清理"改为**临时目录组装 + 整体移入**（`.staging-<包名>` → `mv`） | 初版用 `trap ... RETURN` 做失败清理，但 RETURN trap 在函数**正常返回时也会触发**，实测把刚组装好的产物目录删掉了（只余 tarball）。改为 staging + `mv` 后：失败最多残留 `.staging-*` 临时目录（脚本启动时清理），不会留下半成品，也不会误删 |
| 2 | 校验和工具 | spec 只写了 `sha256sum`，实现加了 `shasum -a 256` 兜底 | 精简环境可能没有 coreutils 的 `sha256sum`；两者输出格式一致，`sha256sum -c` 均可校验 |
| 3 | 选项写法 | 除 `--target <v>` / `--output-dir <v>` 外，额外支持 `--target=<v>` / `--output-dir=<v>` | 常见的 CLI 习惯，代价极小 |
| 4 | rustup 缺失时的目标检查 | spec 未覆盖；实现为"`rustup` 不存在时跳过检查" | 非 rustup 管理的工具链没有 `rustup target` 命令，强行检查会误报"未安装" |
| 5 | release profile 字段 | 与 spec 一致：`lto` / `codegen-units` / `strip`，未动 `opt-level` 与 `panic` | 保持扫描吞吐与既有行为不变 |

## 7. 验证结果

- `cargo build --workspace`：通过（含 `[profile.release]` 后无影响）。
- `cargo test -p f_matcher`：37 个用例全部通过（业务逻辑未改动）。
- `cargo clippy -p f_matcher --all-targets`：无告警。
- 脚本：
  - `bash scripts/build_release.sh --help` → 打印用法与两个支持平台，退出码 0。
  - `bash scripts/build_release.sh` → 出 linux-x86_64 产物。
  - `bash scripts/build_release.sh --all` → 同时出 linux-x86_64 与 linux-aarch64 产物；aarch64 走 `rust-lld` 自包含链接，输出提示"交叉编译：使用 rust-lld 自包含链接"。
  - 错误路径：未知选项 / `--target` 缺值 / 不支持的平台 → stderr 中文提示 + 退出码 1；模拟 `rustup target remove aarch64-unknown-linux-musl` 后脚本给出 `rustup target add aarch64-unknown-linux-musl` 提示并以 1 退出（随后已把 target 装回）。
- 产物核对：
  - `dist/` 结构符合 spec §4（目录 + tar.gz + .sha256，各平台各一份）。
  - `file`：x86_64 产物 `static-pie linked, stripped`；aarch64 产物 `ARM aarch64, statically linked, stripped`。
  - 二进制权限 0755；`sha256sum -c *.sha256` 两个平台均 `OK`。
  - tar.gz 内含一层 `f_matcher-0.1.0-linux-x86_64/` 顶层目录，含二进制、规则文件、README。
- 产物独立可用性：解包到 `/tmp`，**从其它 cwd** 运行包内二进制，`ldd` 显示 `statically linked`；扫描 UTF-8 + GBK 混合目录输出 `a.txt:3`、`sub/c.csv:2`（紧邻排除的 `b.csv` 未命中），退出码 3——默认规则查找命中 exe 同级目录的 `sensitive_rule.txt`。
- `git status --porcelain`：仅显示本次预期改动（`Cargo.toml`、`.gitignore`、`scripts/`、sdlc 文档），`dist/` 已被 `.gitignore` 正确忽略。
