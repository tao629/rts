# 构建方案（Plan）: f_matcher 批量扫描目录下命中敏感关键字的文件

依据：`spec.md`。状态：Draft |

## 1. 改动文件清单与顺序

| 序 | 文件 | 变更 | 说明 |
|----|----|----|----|
| 1 | `Cargo.toml` | 改 | workspace 增加成员 `toolkit/f_matcher`（主仓已有同款未提交改动，本分支一并落地） |
| 2 | `toolkit/f_matcher/Cargo.toml` | 改 | 依赖 `encoding_rs = "0.8"` |
| 3 | `toolkit/f_matcher/src/rule.rs` | 新增 | 规则文件解析（关键字表 + 排除词表）+ 逐行匹配判定 + 单元测试 |
| 4 | `toolkit/f_matcher/src/scanner.rs` | 新增 | 目录遍历（txt/csv、递归、排序）、编码识别（UTF-8 优先 / GB18030 回退）、逐行扫描编排 + 单元测试 |
| 5 | `toolkit/f_matcher/src/cli.rs` | 新增 | 参数解析、用法文本、规则文件路径解析顺序 + 单元测试 |
| 6 | `toolkit/f_matcher/src/main.rs` | 改 | 入口编排：加载规则 → 遍历 → 扫描 → 输出 `绝对路径:行号` → 决定退出码 |
| 7 | `toolkit/f_matcher/README.md` | 新增 | 用法说明（可选交付物） |
| 8 | `sdlc/20261023_敏感文件关键字匹配/spec.md` | 改 | 按实现结果回填偏差 |
| 9 | `sdlc/20261023_敏感文件关键字匹配/plan.md` | 改 | 追加"实现结果与偏差记录" |

依赖方向：`rule`（纯逻辑，无 IO）→ `scanner`（依赖 `rule`）→ `cli`（无依赖）→ `main`（编排全部）。故 3→4→5→6 顺序开发，每步保持 `cargo build` / `cargo test` 可编译通过。

## 2. 关键技术点

### 2.1 编码识别（`encoding_rs`）

- `encoding_rs::UTF_8` / `encoding_rs::GB18030` 均为 `&'static Encoding`；GB18030 是 GBK/GB2312 的超集，覆盖国内业务文件常见编码。
- 判定策略：**单遍优先 + 失败重跑**——先用 UTF-8 解码器扫全文件，一旦解码器报告 `CoderResult::Malformed` 即放弃本次结果、重开文件用 GB18030 重扫。
  - 好处：UTF-8 文件（多数）只读一遍；GBK 文件最多读两遍。
  - 不用"先全量校验再全量解码"的两遍方案，避免重复 IO。
- 解码用 `Decoder::decode_to_string(&src, &mut dst, last)`：
  - `dst` 每次调用传**新建的空 String**（规避 encoding_rs 对非空 dst 的隐含约束），解码结果再追加到跨块缓冲 `pending`。
  - `last=false` 用于流式分块（跨块的多字节序列由 Decoder 内部状态承接），循环结束用 `last=true` 且空 `src` 收尾冲刷。
- 行切分：在 `pending` 上按 `\n` 切分并保留未完行；行尾 `\r` 去掉（兼容 CRLF）；行号从 1 开始。
- BOM：
  - UTF-8 BOM（`EF BB BF`）：UTF-8 解码路径下跳过这 3 字节（否则行首会残留 U+FEFF）。
  - UTF-16 BOM（`FF FE` / `FE FF`）：直接判为不支持 → 告警跳过该文件（计入扫描不完整）。
- 不整文件读入内存：按 8 KiB 分块读，内存占用与文件大小无关。

### 2.2 规则文件解析

- 关键字表：逐行找**去空白后以 `关键字` 开头**的行，取其后内容，剥掉前导的 `:` / `：` 与空白，按 `,` / `，` 切分 → 去空白 → 去空 → 去重（保序）。
- 排除词表：扫描全文，提取成对引号片段（`“…”` / `"…"` / `'…'`），去空白 → 去空 → 去重。找不到引号片段 → 排除词为空（不启用排除规则，不报错）。
- 找不到关键字行或关键字表为空 → 返回错误 → 退出码 1。

### 2.3 匹配判定

- `line.match_indices(kw)` 逐关键字取全部出现位置；对每个位置取其后缀 `&line[pos+kw.len()..]`，若该后缀**以任一排除词开头** → 该次匹配排除。
- 同行多个有效匹配取**字节起始位置最靠前**者（输出只需行号，位置用于确定判定对象）。
- 关键字匹配到第一个有效命中即返回该行行号，扫描器立即终止该文件后续行的处理（命中即停）。

### 2.4 目录遍历

- `fs::read_dir` 递归；**只进入真实目录**（`entry.file_type().is_dir()`，符号链接目录不进入 → 无环路）。
- 扩展名：`path.extension()` 转小写后等于 `txt` / `csv`。
- 收集完成后**按路径排序** → 输出可复现。
- 规则文件自身（canonical path 比对）跳过。

### 2.5 输出与退出码

- `BufWriter<Stdout>`，每命中文件写 `<绝对路径>:<行号>\n`。
- 绝对路径：`std::path::absolute(root)`（不解析符号链接，保留目录树原名）+ 相对路径拼接。
- stdout 写入错误（如 `| head` 提前关闭）→ 静默结束，不 panic。
- 退出码：0 无命中 / 1 参数·目录·规则错误 / 2 扫描不完整（存在读取失败或跳过的不支持编码文件） / 3 有命中且扫描完整。**2 优先于 3**。

## 3. 实现要点

1. `rule.rs`：`Rule { keywords: Vec<String>, exclusions: Vec<String> }`；`Rule::parse(&str) -> Result<Rule, RuleError>`；`Rule::find_hit(&self, line: &str) -> Option<Hit>`（`Hit { keyword: String, position: usize }`）。纯函数，无 IO，便于单测。
2. `scanner.rs`：
   - `collect_files(root: &Path, exclude: &[PathBuf]) -> (Vec<PathBuf>, Vec<ScanIssue>)`——返回待扫文件与遍历告警。
   - `scan_file(path: &Path, rule: &Rule) -> Result<Option<usize>, ScanError>`——返回命中行号（`None` = 未命中）；`ScanError::Malformed` 内部用于触发 GB18030 重扫，对外不暴露。
   - 目录不可读 → 记为 `ScanIssue`（告警）并继续。
3. `cli.rs`：`Cli { dir: PathBuf, rule: Option<PathBuf> }`；`parse_args(&[String]) -> Result<Option<Cli>, CliError>`（`Ok(None)` 表示 `--help`/`--version` 已处理）；`default_rule_path() -> Option<PathBuf>`（exe 同级 → `CARGO_MANIFEST_DIR`）。
4. `main.rs`：编排 + 退出码映射 + stderr 告警输出。

## 4. 风险与对策

| 风险 | 影响 | 对策 |
|----|----|----|
| 关键字与其它词拼接（如"海军医院"）造成误报 | 误报 | 严格紧邻排除词判定（已确认语义） |
| 排除词跨编码边界错位（GBK 半字符被切开） | 漏报/误报 | 在**解码后的 Unicode 文本**上匹配，不在字节层面匹配 |
| GBK 文件被误判为 UTF-8 | 漏报 | 解码器报告 Malformed 即换 GB18030 重扫 |
| 单个超大文件占满内存 | OOM | 分块流式解码，内存与文件大小无关 |
| 目录符号链接成环 | 死循环 | 只进入真实目录，不跟随目录符号链接 |
| 规则文件被自己扫中 | 假阳性 | 显式排除规则文件自身 |
| 输出含敏感内容导致二次扩散 | 合规风险 | 只输出路径与行号 |
| `cargo` 拉取 `encoding_rs` 失败（离线环境） | 构建失败 | 依赖为纯 Rust crate，已确认 crates.io 镜像可达；离线环境需先 vendoring |
| 关键字表变化（规则文件被业务改写） | 行为漂移 | 关键字/排除词全部来自规则文件，代码不硬编码；解析失败时报错而非静默降级 |

## 5. 验证方式

1. `cargo build`：全量通过、无 warning。
2. `cargo clippy --all-targets`：无告警。
3. `cargo test`：覆盖 spec §8.1 的 10 组用例（规则解析、容错、匹配语义、遍历、编码回退、命中即停、CLI 参数）。
4. 手工验证：按 spec §8.2 构造 UTF-8 / GBK 混合目录树，确认输出恰为预期的 `路径:行号` 三行、退出码 3；无命中目录退出码 0；缺目录/坏规则文件退出码 1。

## 6. 实现结果与偏差记录

| 序 | 事项 | 与方案/spec 的偏差 | 原因 |
|----|----|----|----|
| 1 | 关键字数量 | 规则文件实际为 **25** 个关键字（intent/spec 初稿误写 24，已同步更正） | 逐个数过规则文件原文 |
| 2 | 编码识别的实现 | 由计划中的"单遍优先 + Malformed 时重跑"改为**两遍**：第一遍流式校验 UTF-8，第二遍按判定结果解码 | `encoding_rs 0.8.42` 的 `CoderResult` 只有 `InputEmpty`/`OutputFull`，`Malformed` 仅由 `decode_to_string_without_replacement` 报告；改用 `std::str::from_utf8` 做流式 UTF-8 校验（跨块保留至多 3 字节），实现更直接、无输出缓冲容量处理 |
| 3 | GB18030 解码 | 用 `Decoder::decode_to_utf8` + 固定 16 KiB 输出缓冲，处理 `OutputFull` 后重投剩余输入 | `decode_to_string` 以 `String` 容量为输出上限，固定缓冲是 encoding_rs 推荐的流式用法 |
| 4 | 依赖版本 | `encoding_rs` 实际解析为 0.8.42（计划写 0.8） | 镜像源当前版本 |
| 5 | 模块划分 | 与 spec §4 一致：`cli.rs` / `rule.rs` / `scanner.rs` / `main.rs` | 无偏差 |
| 6 | 退出码优先级 | 不完整（2）优先于命中（3） | 与 spec §2 一致，实现按此判定 |

## 7. 验证结果

- `cargo build -p f_matcher`：通过，无 warning。
- `cargo clippy -p f_matcher --all-targets`：无告警（修掉 3 处：`if let` 链可折叠、`manual_contains`、`cloned_ref_to_slice_refs`）。
- `cargo test -p f_matcher`：**37 个用例全部通过**
  - `rule` 解析与匹配 20 例（真实规则文件解析、全角/半角分隔符与冒号、三种引号形式、去重、空项、三类错误、命中/紧邻排除/非紧邻/排除词在前/最早位置/行尾等）
  - `scanner` 14 例（只收 txt/csv 且大小写不敏感、递归子目录、跳过其它扩展名、稳定排序、排除规则文件、UTF-8 命中、GBK 命中、UTF-8 BOM、UTF-16 不支持、命中即停、紧邻排除、无尾换行、CRLF、文件缺失）
  - `cli` 9 例（目录、`--rule` 空格/等号两种写法、选项前置、`--help`/`--version`、缺目录、多目录、未知选项、`--rule` 缺值）
- CLI 手工验证（`/tmp/fm_demo`：UTF-8 + GBK 混合目录树）：
  - 输出恰为 3 行 `/tmp/fm_demo/a.txt:3`、`/tmp/fm_demo/c.txt:2`、`/tmp/fm_demo/sub/e.csv:4`，退出码 3；
  - `b.csv`（`部队医院`）未命中、`d.log`（扩展名不符）被跳过；
  - 真实规则文件下 `军区干休所`（紧邻排除）不命中、`军区` 命中 → 行号 4；
  - 扫描含规则文件自身的目录 → 规则文件被排除，退出码 0（无误报）；
  - 无命中目录 → 退出码 0；目录不存在 / 传文件而非目录 / 规则文件不存在 / 规则文件无关键字 → 退出码 1；
  - 文件权限不足 + UTF-16 文件 → stderr 告警且继续扫描，最终退出码 2；
  - 相对路径入参 → 输出绝对路径；`| head -1` 管道提前关闭 → 静默结束，不 panic。
