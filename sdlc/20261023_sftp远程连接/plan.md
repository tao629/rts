# 构建方案（Plan）: r_connector 基于配置文件连接远端主机并执行指令

依据：`spec.md`。状态：Draft |

## 1. 改动文件清单与顺序

| 序 | 文件 | 变更 | 说明 |
|----|----|----|----|
| 1 | `toolkit/r_connector/Cargo.toml` | 改 | 依赖：`russh`(default-features=false, features=["flate2","ring"])、`tokio`、`log`、`rpassword`、`rts-core`(path) |
| 2 | `base/rts-core/src/logger/log.rs` | 改 | 补齐空壳：`pub fn init(default_level) -> Result<(), FtailError>`，基于已有 `ftail` + `log`；级别由 `RTS_LOG_LEVEL` 覆盖，默认 `Warn` |
| 3 | `toolkit/r_connector/src/config.rs` | 新增 | 配置解析（纯函数）+ 单元测试 |
| 4 | `toolkit/r_connector/src/cli.rs` | 新增 | 命令行参数解析 + 用法文本 + 单元测试 |
| 5 | `toolkit/r_connector/src/hostkey.rs` | 新增 | 主机密钥校验/TOFU（基于 `russh::keys` known_hosts 能力）+ 单元测试 |
| 6 | `toolkit/r_connector/src/session.rs` | 新增 | SSH 连接、密码认证、逐条 exec 指令 |
| 7 | `toolkit/r_connector/src/report.rs` | 新增 | 回显格式（`>>> [i/n]` / 原样输出 / `<<< exit=` / 汇总行） |
| 8 | `toolkit/r_connector/src/main.rs` | 改 | 入口编排、退出码映射 |
| 9 | `sdlc/20261023_sftp远程连接/spec.md` | 改 | 按实现结果回填偏差（如指令输出改为流式转发） |

依赖方向：`config`/`cli`/`report` 为纯逻辑，不依赖 russh；`hostkey` 依赖 russh；`session` 依赖 russh + `hostkey`；`main` 编排全部。故 3→4→5→6→7→8 顺序开发，每步 `cargo build` / `cargo test` 保持可编译。

## 2. 关键技术点（已核实到的 API 事实）

- `russh 0.64.1` 必须启用 `ring` 或 `aws-lc-rs` 之一（否则 `compile_error!`）。选 `ring`：纯 Rust 生态、不需要 cmake；
  同时关掉默认的 `aws-lc-rs`（需 cmake + 编译 C）与 `rsa`（本次无私钥鉴权）。协商时优先 `chacha20-poly1305`（纯 Rust 实现）。
- 客户端：`russh::client::connect(Arc<client::Config>, (host, port), handler)`；`Config` 含 `inactivity_timeout` / `keepalive_interval` / `keepalive_max` / `nodelay`。
- 认证：`handle.authenticate_password(user, password)` 返回 `AuthResult`，检查 `.success()`。
- 主机密钥：在 `client::Handler::check_server_key(&mut self, &PublicKeyOrCertificate)` 回调里校验，返回 `bool`。
  `PublicKeyOrCertificate` 变体：`PublicKey { key, hash_alg }` / `Certificate(cert)`。
- known_hosts：`russh::keys::known_host_keys_path(host, port, path)` 取匹配条目；
  `russh::keys::learn_known_hosts_path(host, port, pubkey, path)` 写入。
  端口非 22 时以 `[host]:port` 匹配与记录（与 OpenSSH 一致）；哈希主机名条目由 russh 内部用 HMAC-SHA1 校验。
  → 因此**无需**额外引入 `hmac`/`sha1` 依赖。
- 指令执行：`handle.channel_open_session()` → `channel.exec(true, cmd)` → 循环 `channel.wait()` 收
  `ChannelMsg::Data`（stdout）/ `ChannelMsg::ExtendedData{ext:1}`（stderr）/ `ChannelMsg::ExitStatus`。
- 密码输入：`rpassword::prompt_password` 走 `/dev/tty` 且不回显。

## 3. 实现要点

1. **配置解析**（`config.rs`）
   - 有效行 = 去掉空行与 `#` 注释行后的行；前 3 条为头部（协议/连接/账号），其余为指令。
   - 头部行允许 `协议:` / `连接:` / `账号:` 前缀（中英文冒号），解析前剥除；指令行不剥除。
   - 连接信息：`[addr]:port` / `host:port` / 仅 host（端口 22）；无方括号的含冒号串按 IPv6 处理（端口 22）。
   - 账号密码：按**第一个**逗号切分，用户名不可为空，密码可为空（→ 交互补输）。
2. **主机密钥**（`hostkey.rs`）
   - 无匹配条目 → 打印指纹 + 写入 known_hosts → 放行（TOFU）；写文件失败 → 拒绝连接。
   - 有匹配条目且公钥相同 → 放行；不同 → 拒绝并打印已记录/本次指纹。
   - 校验发生在 `check_server_key` 回调内（同步文件 IO，文件很小；若后续 known_hosts 变大再考虑 `spawn_blocking`）。
3. **连接与执行**（`session.rs`）
   - 连接整体 10s 超时（`tokio::time::timeout`）；单条指令默认 300s 超时，可用 `RTS_COMMAND_TIMEOUT`（秒）覆盖。
   - 每条指令独立开 channel；stdout 数据边收边转发到本地 stdout，stderr 转发到本地 stderr（流式，避免大输出占内存）。
   - 单条指令失败不中断后续；连接级错误（通道开不了/连接断开）→ 终止并以退出码 5 收尾。
4. **回显**（`report.rs`）
   - `>>> [i/n] <cmd>` → 原始输出 → `<<< exit=<code>`；出错为 `<<< error: <原因>`。
   - 末尾汇总：`执行完成: 共 n 条指令, 成功 s 条, 失败 f 条`。
5. **退出码**（`main.rs`）：0 正常完成 / 1 参数与配置错误 / 2 连接失败 / 3 认证失败 / 4 主机密钥不匹配 / 5 执行期致命错误。

## 4. 风险与对策

| 风险 | 影响 | 对策 |
|----|----|----|
| `ring` 需要 C 编译器（cc） | 目标机缺 cc 时构建失败 | 备选 `aws-lc-rs`（需 cmake，更重）；两者都不可用时退回“调用系统 ssh”方案（本次不做） |
| known_hosts 中存在本工具不认识的条目（如 `@cert-authority`） | 扫描报错 | russh 的扫描实现只解析**主机名匹配**的条目，未匹配条目不影响；无法解析的匹配条目 → 明确报错而非静默忽略 |
| 远端命令输出很大 | 内存暴涨 | 流式转发，不缓冲 |
| 远端 shell 与本地 shell 语义差异 | 命令行为不符预期 | 指令原样透传，`--help` 中明确说明由远端 shell 解释 |
| 密码出现在进程列表/日志 | 凭据泄露 | 密码只从配置文件读入内存，不写日志；日志中不打印连接参数里的密码；配置支持留空走交互输入 |
| 指令卡死不返回 | 工具挂住 | 单条指令超时（默认 300s），超时按该条失败处理并继续 |

## 5. 验证方式

1. `cargo build` 全量通过。
2. `cargo test`：`config` 解析用例（含缺行/注释/标签前缀/端口缺省/IPv6/密码含逗号/非法协议）、`cli` 参数用例、`hostkey` 三路径（首次记录 → 命中 → 变更拒绝，用临时目录）。
3. 手工：以本机 sshd 为远端，跑通"读配置 → 连接 → 依次执行 → 回显与退出码 → 退出"；复跑验证 known_hosts 命中；篡改 known_hosts 验证拒绝；错误密码验证认证失败文案与退出码。

## 6. 实现结果与偏差记录

| 序 | 事项 | 与方案/spec 的偏差 | 原因 |
|----|----|----|----|
| 1 | 指令输出 | 由"先收集再回显"改为**流式转发** | 大输出（如 `cat` 大日志）不占内存；回显语义不变 |
| 2 | 通道收尾 | 收到退出码后加 2 秒宽限等待远端关闭通道 | 部分服务端不回 `Close`，否则会一直等到指令超时 |
| 3 | 管道关闭 | stdout 被下游关闭时静默退出 | Rust 默认在 broken pipe 上 panic（`\| head` 场景实测复现） |
| 4 | 环境变量 | 新增 `RTS_LOG_LEVEL` / `RTS_KNOWN_HOSTS` / `RTS_COMMAND_TIMEOUT` | 便于测试与按环境调整超时/信任文件 |
| 5 | known_hosts 匹配 | 比较公钥本体（`key_data`）而非整个 `PublicKey` | `PublicKey` 的 `PartialEq` 含 comment，而 known_hosts 回读时 comment 丢失，会导致误判为"密钥变更" |
| 6 | 依赖 | 未引入 `hmac`/`sha1`；新增 dev 依赖 `rand` | 哈希主机名校验由 `russh::keys::known_hosts` 内置实现；`rand` 仅用于测试与 mock 服务端生成主机密钥 |
| 7 | 验证方式 | 本机无 sshd，改用 `examples/mock_ssh_server.rs` + 进程内集成测试 | WSL 非 root 无法起 sshd；mock 服务端不执行任何本地命令 |

## 7. 验证结果

- `cargo build`：通过，无 warning。
- `cargo clippy --all-targets`：无告警。
- `cargo test`：30 个用例全部通过
  - `config` 解析 18 例（规范格式、协议大小写、端口缺省/IPv6/非法端口、密码含逗号/留空、注释空行、标签前缀、缺行、非法协议、指令为空等）
  - `cli` 参数 5 例
  - `hostkey` 3 例（首次记录 → 命中 → 变更拒绝；非默认端口独立记录）
  - `session` 集成 3 例（进程内 mock SSH 服务端：指令执行与退出码、known_hosts 复用、错误密码 → 认证失败）
  - `rts-core` logger 2 例
- CLI 手工验证（对 mock 服务端）：首次连接打印并记录指纹 → 三条指令按序执行、stdout/stderr 分离、退出码正确 → 汇总行正确 → 进程退出码 0；第二次运行不再提示"首次信任"；错误密码 → 提示认证失败且退出码 3；配置缺行/非法协议 → 退出码 1；端口未监听 → 退出码 2。
