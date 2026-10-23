# 构建方案（Plan）: rts-core 基于 tracing 的滚动归档日志模块

依据：`spec.md`（Draft）。状态：Draft |

## 1. 改动文件清单与顺序

| 序 | 文件 | 变更 | 说明 |
|----|----|----|----|
| 1 | `Cargo.toml` | 改 | `[workspace.dependencies]` 增加 `tracing` / `tracing-subscriber` / `tracing-appender` / `tracing-log` / `flate2` / `chrono` |
| 2 | `base/rts-core/Cargo.toml` | 改 | 移除 `ftail`；新增上述依赖（`log` 保留，用于 facade 桥接的级别换算） |
| 3 | `base/rts-core/src/logger/config.rs` | 新增 | `LogConfig`（`Option<T>` 字段）+ `ResolvedConfig` + 环境变量解析 + 默认值 |
| 4 | `base/rts-core/src/logger/writer.rs` | 新增 | 滚动写入器：按天/按大小判定、GZ 归档、序号分配、过期清理；实现 `io::Write` |
| 5 | `base/rts-core/src/logger/log.rs` | 改 | `init()` 装配 `tracing_subscriber` + `LogTracer` 桥接；级别解析；`LoggerGuard` |
| 6 | `base/rts-core/src/logger.rs` | 改 | 声明子模块 `config` / `writer`，导出公共 API |
| 7 | `base/rts-core/src/lib.rs` | 改 | 删除示例用的 `add` 函数与测试（模块已提供实质内容） |
| 8 | `base/rts-core/tests/init.rs` | 新增 | 集成测试：facade + tracing 双路落盘、滚动、GZ 归档 |
| 9 | `base/rts-core/examples/log_demo.rs` | 新增 | 演示程序：反复写日志，观察滚动与归档（人工验证入口） |
| 10 | `toolkit/r_connector/src/main.rs` | 改 | 适配新的 `init` 签名与 guard；日志宏保持 `log::` 不变；拆分 `run` / `run_inner` 以便显式释放 guard |
| 11 | `sdlc/20261023_rts-core日志模块/{intent,spec,plan}.md` | 改 | 回填发起人答复、实现偏差与验证结果 |

依赖方向：2 依赖 1；4 依赖 3；5 依赖 3、4；10 依赖 5。按 1→2→3→4→5→6→7→8→9→10 顺序开发，每步保持 `cargo build --workspace` 通过。

## 2. 关键技术点（已核实的事实）

- **`tracing-appender` 不提供按大小滚动**：`rolling` 模块只有 `minutely` / `hourly` / `daily` / `never`，且不压缩、不清理。因此滚动写入器自行实现。
- **`NonBlockingBuilder::finish`**：签名 `finish<T: Write + Send + 'static>(self, writer: T) -> (NonBlocking, WorkerGuard)`；
  `NonBlocking` 自身实现 `MakeWriter`（`make_writer` 返回 `NonBlocking` 克隆）。
  因此**自定义滚动写入器直接实现 `io::Write`**（而不是 `MakeWriter`），再交给 `finish` 包装。
- **`NonBlocking` 的写入路径**：`impl Write for NonBlocking` 把字节以 `Msg::Line(Vec<u8>)` 投进有界 channel；
  `lossy(true)` 时 channel 满则丢弃并累加 `dropped_lines()` 计数（默认 `lossy = true`，`buffered_lines_limit` 默认 1024）。
- **`WorkerGuard` 必须存活**：`Drop` 时发 shutdown 信号并等待 worker flush（最多 1s）。guard 析构后工作线程退出，后续日志丢失 → `init()` 必须返回 guard。
- **滚动/压缩发生在 worker 线程**：`NonBlocking` 的 worker 线程调用内层 writer 的 `write`，因此 GZ 压缩、过期清理都在 worker 线程执行，不阻塞业务线程。
- **`LogTracer` 初始化不 panic**：`tracing_log::LogTracer::builder().with_max_level(..).init() -> Result<(), log::SetLoggerError>`
  （内部 `log::set_boxed_logger` + `log::set_max_level`）。
- **不直接用 `SubscriberInitExt::try_init()`**：它内部会 `LogTracer::builder().with_max_level(LevelFilter::current().as_log()).init()`，
  其 max level 依赖 `LevelFilter::current()`（全局 MAX_LEVEL，由已注册 dispatcher 的 `max_level_hint` 推出），
  推导链较长且依赖 `Layer::max_level_hint` 默认返回 `None`。改为手工装配：
  `tracing::subscriber::set_global_default(subscriber)` → 再显式 `LogTracer::builder().with_max_level(配置的级别).init()`，
  使 `log` facade 的 max level 与本模块配置的级别严格一致。
- **`tracing_core::LevelFilter` 语义**：`LevelFilter::WARN` 表示"WARN 及以上"；含 `OFF` 变体（`tracing::Level` 没有 `OFF`），
  故对外的级别类型用 `LevelFilter` 而非 `Level`。
- **`tracing_log` 重导出 `log`**：`tracing_log::log::LevelFilter` 即 `log::LevelFilter`；rts-core 直接声明 `log` 依赖以使用 `log::LevelFilter`。
- **本地已缓存的依赖版本**：`tracing 0.1.44` / `tracing-subscriber 0.3.23` / `tracing-appender 0.2.5` / `tracing-log 0.2.0` / `flate2 1.1.10` / `chrono 0.4.45`，构建可离线进行。

## 3. 实现要点

### 3.1 `config.rs`

```rust
pub struct LogConfig {
    pub dir: Option<PathBuf>,
    pub file_name: Option<String>,
    pub archive_dir: Option<String>,
    pub max_size: Option<u64>,
    pub max_age_days: Option<u32>,
    pub level: Option<LevelFilter>,
    pub console: Option<bool>,
}
```

- `Default` 全部为 `None`。
- 构建器方法：`dir()` / `file_name()` / `archive_dir()` / `max_size()` / `max_age_days()` / `level()` / `console()`。
- `resolve() -> ResolvedConfig`：逐字段 **显式 → 环境变量 → 默认值** 解析。
  - 环境变量：`RTS_LOG_DIR` / `RTS_LOG_MAX_SIZE` / `RTS_LOG_MAX_AGE_DAYS` / `RTS_LOG_LEVEL` / `RTS_LOG_CONSOLE`。
  - `RTS_LOG_MAX_SIZE` 支持带单位的字符串（`300M` / `300MB` / `1G` / 纯字节数）；解析失败 → 回退默认 + 告警。
  - `RTS_LOG_LEVEL` 支持 `off/error/warn/info/debug/trace`（大小写不敏感）；非法 → 回退默认 + 告警。
- 默认值：目录 `./logs`、文件名 `app.log`、归档目录 `archive`、`max_size = 300 * 1024 * 1024`、`max_age_days = 7`、级别 `INFO`、控制台 `true`。
- `ResolvedConfig` 为内部结构（字段全部为具体值），供 `writer` 与 `log` 使用。

### 3.2 `writer.rs`

状态结构（全部在 `Mutex` 内，实际只有 worker 线程访问，锁仅用于满足 `Write` 的 `&mut self` 语义与潜在的直接调用）：

```rust
struct RollingState {
    dir: PathBuf,
    archive_dir: PathBuf,
    file_path: PathBuf,
    file_name: String,
    max_size: u64,
    max_age_days: u32,
    file: File,
    file_date: NaiveDate,   // 当前活动文件内容所属的自然日
    file_size: u64,
    date_checked_at: Instant, // 上次刷新"今天"的时刻
}
```

关键方法：

| 方法 | 职责 |
|----|----|
| `RollingWriter::new(cfg)` | 建目录 → 启动清理 → 按 spec §4.2 打开/追加/归档活动文件 |
| `write(buf)` | 刷新日期 → 判定跨天/超限 → 必要时滚动 → 写入 |
| `refresh_date()` | `date_checked_at.elapsed() >= 1s` 时重取 `Local::now().date_naive()`；与 `file_date` 不同则触发跨天滚动 |
| `rotate(reason)` | flush → 关闭 → GZ 归档 → 删原文件 → 打开新活动文件 → 过期清理 |
| `next_seq_for(date)` | 扫描 `archive/` 中该日期的归档，取最大序号 + 1（启动与每次滚动都用它，避免计数器漂移与覆盖既有归档） |
| `archive_name(date, seq)` | `format!("{file_name}.{date}.{seq}.gz")` |
| `compress(src, dst)` | `flate2::write::GzEncoder` 压缩；失败 → 删除半成品、保留原文件、返回 `Err` |
| `cleanup_expired()` | 删除归档日期 `< today - (max_age_days - 1)` 的 `.gz`；无法解析日期的文件跳过不动 |

细节：

- **滚动判定在写入前**：`file_size > 0 && file_size + buf.len() > max_size` → 滚动。
  `file_size == 0` 时不因大小滚动（单条日志大于上限时完整写入，写完下一次滚动）——对应 spec §4.3。
- **跨天滚动**：`refresh_date` 发现今天 ≠ `file_date` → 以 `file_date` 为归档日期滚动，随后 `file_date = 今天`、`file_size = 0`。
- **序号**：每次滚动调用 `next_seq_for(file_date)` 取序号，天然做到"同一天内 0,1,2… 递增、跨天归零、进程重启不覆盖"。
- **启动时活动文件处理**（spec §4.2）：`app.log` 存在时按 mtime 判定日期；等于今天 → 追加（`OpenOptions::append(true)`），并按 mtime 记 `file_size`；
  小于今天 → 先按滚动逻辑归档到 `archive/`（日期取 mtime 日期），再新建今天的 `app.log`。
- **日期刷新节流**：1 秒一次，避免每写一条日志都取时钟。
- **错误处理**：滚动失败（GZ 失败、打开新文件失败）→ 记 `eprintln!` 告警并**继续使用当前文件**（不丢日志）；清理失败 → 仅告警。**任何路径都不 panic**。
- `impl Write for RollingWriter`：持锁后调用 `RollingState::write`；锁中毒时用 `unwrap_or_else(|e| e.into_inner())` 恢复。

### 3.3 `log.rs`

```rust
pub fn init(config: LogConfig) -> Result<LoggerGuard, LoggerError>;
pub struct LoggerGuard { _worker: WorkerGuard, _non_blocking: NonBlocking }  // drop 时 flush
```

装配顺序：

1. `config.resolve()` → `ResolvedConfig`；
2. `RollingWriter::new(&resolved)`（建目录、启动清理、打开活动文件）；
3. `NonBlockingBuilder::default().lossy(true).buffered_lines_limit(1024).finish(writer)` → `(non_blocking, worker_guard)`；
4. 构造 subscriber：
   - 文件层 `fmt::layer().with_writer(non_blocking).with_ansi(false)`（文件里纯文本）；保留 target / file / line / thread id。
   - 控制台层 `fmt::layer().with_writer(std::io::stderr)`（ANSI 彩色，默认开启，可关闭）。
   - 全局级别过滤：`.with(LevelFilter)`。
5. `tracing::subscriber::set_global_default(subscriber)` → 失败返回 `LoggerError::AlreadyInitialized`；
6. `tracing_log::LogTracer::builder().with_max_level(配置的级别 → log::LevelFilter).init()` → 失败返回 `LoggerError::LogTracer`。
   （步骤 6 让 `log::info!` 等 facade 调用进入同一份日志文件，并按同一级别过滤。）
7. 返回 `LoggerGuard`。

`LoggerError`：`std::io::Error`（目录/文件）、`AlreadyInitialized`、`LogTracer(String)`、`InvalidConfig(String)`；
实现 `std::error::Error` + `Display`。

`LevelFilter` ↔ `log::LevelFilter` 换算函数 `level_to_log(LevelFilter) -> log::LevelFilter`，以及给老调用方的 `level_from_log(log::LevelFilter) -> LevelFilter`。
`rts_core::logger` 重导出 `LevelFilter`（= `tracing::level_filters::LevelFilter`）。

### 3.4 `r_connector` 适配

```rust
// 旧
use log::{LevelFilter, info};
rts_core::logger::log::init(LevelFilter::Warn)?;

// 新
use log::info;                      // facade 宏保持不变
use rts_core::logger::LevelFilter;  // 级别类型换成 tracing 的
let _guard = rts_core::logger::log::init(
    rts_core::logger::LogConfig::default().level(LevelFilter::WARN),
)?;
```

`init` 失败时的处理沿用现有风格（打印错误并退出）。

## 4. 风险与对策

| 风险 | 影响 | 对策 |
|----|----|----|
| `NonBlocking` 的 `WorkerGuard` 被提前 drop | 进程还在跑但日志不再落盘 | `init` 返回 `LoggerGuard`，r_connector 用 `let _guard = ...` 持有到进程结束；文档明确说明 |
| 异步缓冲在 `kill -9` 时丢失 | 极端情况下丢最后若干条日志 | 已知代价（发起人选择异步非阻塞）；通过 `dropped_lines()` 可观测丢弃量 |
| 进程重启时活动文件归属日期判断错 | 昨天的尾巴被算进今天的文件 | 用 mtime 判定；mtime < 今天则先归档再新建 |
| 归档序号与既有文件冲突（重启后从 0 开始） | 覆盖当天已有归档 | 每次滚动都扫描归档目录取"该日期最大序号 + 1"，不依赖内存计数器 |
| GZ 压缩失败被误删原文件 | 丢日志 | 压缩失败时保留原活动文件，仅告警 |
| 清理误删非归档文件 | 丢数据 | 只处理 `archive/` 目录内、且文件名能解析出日期的 `.gz`；其余跳过 |
| 清理逻辑把"未来日期"的归档删掉 | 误删 | 仅删除日期 `< cutoff` 的文件，未来日期文件保留 |
| 单条日志大于 `max_size` | 文件超出上限 | 允许超出（不截断日志内容），写完即滚动；spec §4.3 已声明 |
| `log::` facade 与 `tracing::` 双重记录 | 同一事件写两遍 | 只在 rts-core 内安装一次 `LogTracer`；调用方不同时用两套宏写同一条日志 |
| `tracing` 全局 subscriber 只能设置一次 | 重复 `init` 报错 | `init` 返回 `LoggerError::AlreadyInitialized`，不 panic |
| 新增依赖破坏离线构建 | 无法构建 | 依赖版本已核对本地缓存；构建后确认 `Cargo.lock` 已更新且 `--offline` 可通过 |
| `cargo clippy` 告警 | CI 不通过 | 实现阶段逐条清理 |

## 5. 验证方式

1. `cargo build --workspace`（含 `--offline` 验证离线可构建）。
2. `cargo test --workspace`：
   - 单元测试（`writer.rs` 内）：滚动触发、序号递增与跨天归零、GZ 可解压还原、过期归档清理、活动文件存活、无法解析日期的文件不被删。
   - 集成测试（`tests/logger.rs`）：`max_size = 1 KiB` 触发滚动 → `archive/` 出现 `app.log.<今天>.0.gz`、`.1.gz`；`log::info!` 与 `tracing::info!` 都进文件；启动清理删掉构造出的过期归档。
3. `cargo clippy --workspace --all-targets` 无告警。
4. 手工验证（`r_connector`）：
   - 默认配置启动 → 生成 `logs/app.log`；
   - `RTS_LOG_MAX_SIZE=1M` 制造多次滚动 → `archive/` 内序号连续、`zcat` 内容完整；
   - `RTS_LOG_MAX_AGE_DAYS=1` + 构造过期归档 → 重启后过期文件被删、当天文件保留；
   - `RTS_LOG_LEVEL=warn` → `log::info!` 不落盘、`log::warn!` 落盘。
5. `git status --porcelain` 仅显示预期改动。

## 6. 实现结果与偏差记录

| 序 | 事项 | 与方案/spec 的偏差 | 原因 |
|----|----|----|----|
| 1 | 归档落地顺序 | 由 spec 的"压缩 → 删除原文件"改为 **"先 `rename` 进归档目录 → 再压缩 → 删除未压缩中间文件"** | rename 只改目录项、不占额外空间，磁盘写满时也能成功；且压缩失败时日志内容仍完整留在归档目录里，只是没有 `.gz` 后缀。压缩失败的降级产物命名为 `app.log.<日期>.<序号>`（无 `.gz`），清理逻辑对两种形式都能解析 |
| 2 | 归档序号分配 | 由 spec 的"每次滚动都扫描归档目录取最大序号+1"改为 **启动时扫描一次 + 内存递增 + 跨天时重新扫描** | 实测每次滚动全量扫描在归档数变多后是 O(n²)；改为缓存计数器后滚动开销与归档数量无关。跨天与进程重启仍会重新扫描，语义不变 |
| 3 | 时间戳时区 | spec 未规定；实现采用 **本地时区**（`chrono::Local`，`%Y-%m-%dT%H:%M:%S%.6f%:z`） | 默认的 UTC 时间戳与归档文件名里的本地日期对不上，跨时区排查时容易错位。`tracing-subscriber` 自带的 `local-time` feature 需要给 `time` crate 加 `--cfg unsound_local_offset`（要动全局 RUSTFLAGS，代价太大），改用已依赖的 `chrono` 自定义 `FormatTime` |
| 4 | `tracing-subscriber` feature | 额外启用 `chrono` feature | `local-time` feature 依赖 `time` crate 的本地偏移能力（需 `unsound_local_offset` cfg），`chrono` feature 则直接提供 `ChronoLocal` 格式化器，且 chrono 本就是本 crate 的依赖 |
| 5 | 清理时的未知文件 | spec 写"记录一次告警"，实现为 **静默跳过** | 清理在每次滚动后都会扫一遍目录，重复告警太吵；无法解析日期的文件一律不删 |
| 6 | `format.rs` 子模块 | spec §2.2 列了这个文件，实现 **未新建**，相关内容并入 `log.rs` | 只有一个本地时间格式化函数，单独开文件过重 |
| 7 | 新增示例程序 | spec §2.2 未列；实现新增 `base/rts-core/examples/log_demo.rs` | 人工验证"滚动 + GZ 归档 + facade 兼容"需要能反复写日志的入口，比手工拼命令可靠 |
| 8 | `r_connector` 的 `run` 拆分 | plan §3.4 未提；实现把原 `run` 拆成 `run`（持有 guard）+ `run_inner`（原业务逻辑） | `main` 用 `std::process::exit` 结束进程，而 `process::exit` 不执行析构——guard 若在 `run` 返回时才析构，异步缓冲来不及 flush。拆分后可在 `run_inner` 返回后显式 `drop(guard)` |
| 9 | `log` 依赖 | spec §2.2 一度写成移除 `log` 直接依赖，最终 **保留** | facade 桥接需要 `log::LevelFilter` 来设置 `log` 侧的最大级别（否则 `log::info!` 在级别为 WARN 时仍会被完整格式化后才丢弃） |
| 10 | `SubscribertInitExt::try_init()` | plan §2 说不用它，**实现确认不用** | 它内部用 `LevelFilter::current()` 推导 `log` 侧 max level，而该值来自已注册 dispatcher 的 `max_level_hint`（`Layer` 默认返回 `None`），推导链不可控。改为手工装配：`set_global_default` → 再显式 `LogTracer::builder().with_max_level(配置的级别).init()` |

## 7. 验证结果

### 自动化

- `cargo build --workspace`：通过。
- `cargo test --workspace`：**83 个用例全部通过**
  - `rts-core` 单元测试 16 个（配置解析 6 + 级别换算 2 + 滚动写入器 8）；
  - `rts-core` 集成测试 1 个（`tests/init.rs`：facade + tracing 双路落盘、滚动、GZ 归档）；
  - 文档测试 2 个；
  - `f_matcher` 37 个、`r_connector` 27 个（未改动业务逻辑，回归通过）。
- `cargo clippy --workspace --all-targets`：仅剩 `config/configure.rs` 里 `struct Configure` 的 `dead_code` 告警（**改动前就存在**，不属本次范围）。
- `rustfmt`：仅对本次改动过的文件格式化；仓库整体本就不是 fmt-clean（`f_matcher` 有 3 个文件存在历史 diff），未做无关改动。

### 单元测试覆盖的关键行为

| 行为 | 用例 |
|----|----|
| 写满触发滚动，归档序号从 0 连续递增 | `rolls_by_size_and_numbers_archives_from_zero` |
| 归档是合法 gzip，解压后内容完整 | `archive_is_valid_gzip_keeping_original_content` |
| 过期归档被删、当天归档与无法解析日期的文件保留 | `cleanup_removes_expired_archives_and_keeps_recent_ones` |
| 跨天未滚动就退出：遗留活动文件按 mtime 归档到它所属的那一天 | `startup_archives_stale_active_file_from_previous_day` |
| 进程重启后序号接着已有归档继续，不覆盖 | `restart_continues_sequence_instead_of_overwriting` |
| 归档名解析（`.gz` 与未压缩两种形式） | `parse_archive_handles_gz_and_plain` / `parse_archive_rejects_unrelated_names` |
| 级别解析、配置文件大小解析、优先级 | `parse_level_*` / `parse_size_*` / `default_config_resolves_to_builtin_defaults` / `explicit_config_wins_over_defaults` |
| `log` facade 与 `tracing` 宏都落盘 | `tests/init.rs::writes_to_file_and_rolls_into_gz_archives` |

### 人工验证

- `RTS_LOG_DIR=/tmp/rts-log-demo RTS_LOG_MAX_SIZE=10K cargo run -p rts-core --example log_demo`：
  - 生成 `archive/app.log.2026-10-23.{0,1,2,3}.gz`，序号从 0 连续；
  - `file` 判定为 `gzip compressed data`，`zcat` 解压后内容与原文一致（首条为"第 0 条日志"，归档 0 共 105 行）；
  - 活动文件 `app.log` 保留最后若干条，含 `log::info!` 写入的"这条日志走 log facade"——facade 桥接生效；
  - 时间戳为本地时区 `2026-10-23T22:03:39.193724+08:00`，与归档日期一致。
- 保留策略：`RTS_LOG_MAX_AGE_DAYS=2` 下，构造的 `app.log.2026-10-01.0.gz` 被删除，`app.log.2026-10-22.0.gz`（昨天）保留。
- 序号连续性：`RTS_LOG_MAX_SIZE=1K` 跑出 54 个归档，序号经脚本校验为 **连续 0..53 无缺口**。
- `r_connector --help`：正常输出用法，并在 `RTS_LOG_DIR` 下创建 `app.log` 与 `archive/`。

