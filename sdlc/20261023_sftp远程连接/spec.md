# 需求规格说明书（Spec）: r_connector 基于配置文件连接远端主机并执行指令

依据：`intent.md`（Finalize）。状态：Draft，待发起人评审 |

## 1. 范围

交付一个独立命令行工具 `r_connector`：读取一个由命令行参数指定的配置文件，解析出协议、连接信息、账号密码与待执行指令，建立到远端主机的 SSH 连接，顺序执行指令并回显结果，执行完毕后退出。

不在本次范围：文件传输（SFTP 上传下载）、交互式 REPL、私钥/证书鉴权、多主机并发、图形界面、rts-center 集成。

## 2. 命令行接口

```text
r_connector <config-file>      # 必填，唯一位置参数：配置文件路径
r_connector --help             # 打印用法与配置格式示例后退出 0
r_connector --version          # 打印版本后退出 0
```

- 参数个数 != 1 或出现未知参数：打印用法到 stderr，退出码 1。
- 配置文件路径不可读/不存在：提示原因，退出码 1。
- 参数解析手写实现，不引入 clap（保持依赖树精简）。

## 3. 配置文件格式

### 3.1 规范形式

```text
sftp
192.168.1.10:2222
root,Passw0rd
uname -a
ls -l /var/log
systemctl status nginx
```

| 行 | 含义 | 格式 | 必填 |
|----|------|------|------|
| 1 | 协议 | `sftp` 或 `ssh`，大小写不敏感 | 是 |
| 2 | 连接信息 | `host:port`，`port` 可省略（默认 22）；IPv6 用 `[addr]:port` | 是 |
| 3 | 账号密码 | `user,password`，密码可为空 | 是 |
| 4..N | 待执行指令 | 每行一条，原样透传给远端 shell | 否（可为空） |

### 3.2 解析规则

1. 文件按 `\n` 切分为行，兼容 `\r\n`（去掉行尾 `\r`）。
2. **跳过规则**：空行（去空白后为空）跳过；以 `#` 开头的行跳过。
3. **标签前缀**：前三个头部行允许带可选的 `标签:` 前缀（`协议:` / `连接:` / `账号:`，中英文冒号均可），解析时剥除；指令行不做此处理（避免误伤 `echo a:b`）。
4. 头部行 = 前 3 个"有效行"（跳过规则生效后）。有效行不足 3 个 → 报错：明确指出缺哪一行。
5. 协议行取值不在 `{sftp, ssh}` 内 → 报错并列出支持的值。
6. 连接信息行：
   - 形如 `[addr]:port` → 主机 `addr`，端口 `port`。
   - 形如 `host:port` → 按最后一个 `:` 切分；`host` 含 `:` 但不带方括号时，视为 IPv6（此时端口缺省 22）。
   - 无 `:` → 主机为整行，端口 22。
   - 端口必须是 1..=65535 的十进制整数，否则报错。
7. 账号密码行：以**第一个**逗号切分，左为用户名（不可为空），右为密码（可为空，可含逗号）。无逗号 → 报错。
8. 指令行：去首尾空白后原样保留（行内内部空白保留），作为一条远端命令。行内不做 shell 转义——执行语义是**远端 shell**。

### 3.3 示例（容错形式）

```text
# r_connector 配置文件
协议: sftp
连接: 192.168.1.10:2222
账号: root,
uname -a
```

### 3.4 密码留空的处置

账号行密码为空时，工具在连接前于本地终端交互式提示输入密码（**不回显**，不写日志，不落盘）。若当前环境无 TTY（stdin 非终端），报错退出。

## 4. 模块划分

| 文件 | 职责 | 关键类型 |
|----|----|----|
| `src/main.rs` | 入口：解析参数 → 编排各阶段 → 决定退出码 | — |
| `src/cli.rs` | 命令行参数解析与用法文本 | `Cli { config_path }` |
| `src/config.rs` | 配置文件解析（纯函数，无 IO 副作用之外） | `Config { protocol, host, port, user, password, commands }`、`Protocol` |
| `src/hostkey.rs` | 主机密钥校验与 TOFU 记录 | `HostKeyVerifier` |
| `src/session.rs` | SSH 连接、密码认证、逐条执行指令 | `SshSession` |
| `src/report.rs` | 结果回显格式 | `CommandOutcome` |

错误类型：各模块自定义 `enum ...Error`（实现 `Display`），`main` 统一转成退出码与 stderr 提示；不引入 `thiserror`/`anyhow`。

## 5. 行为规格

### 5.1 连接与认证

- 使用 `russh`（纯 Rust SSH 客户端，crypto 后端用 `ring`，无系统 C 库依赖）。
- 连接参数：`client::Config`，`inactivity_timeout = 30s`、`keepalive_interval = 15s`、`keepalive_max = 3`、`nodelay = true`。
- 连接超时：TCP 连接 + 握手整体超时 **10 秒**（`tokio::time::timeout` 包裹），超时按"网络不可达/握手超时"报错。
- 认证：仅密码认证（`authenticate_password`）。失败区分：网络错误 / 认证被拒（服务端返回失败）两类文案。
- 协议行取 `sftp` 与 `ssh` **行为一致**：均建立 SSH 连接，指令通过 SSH `exec` 通道执行（见 §6 冲突澄清 C1）。

### 5.2 主机密钥（TOFU）

使用 `russh::keys` 提供的 known_hosts 能力（文件位置 `~/.ssh/known_hosts`）：

1. 服务端在 `check_server_key` 回调中提交公钥。
2. 用 `russh::keys::known_host_keys(host, port)` 取出本机已记录的匹配条目：
   - **有匹配条目且公钥相同** → 放行。
   - **有匹配条目但公钥不同** → 拒绝连接，报"主机密钥已变更"并打印已记录指纹与本次指纹，退出码 4。
   - **无匹配条目**（首次连接）→ 打印本次主机密钥指纹（SHA256），写入 `~/.ssh/known_hosts` 后放行；写文件失败则拒绝连接（不静默跳过校验）。
3. 端口非 22 时以 `[host]:port` 形式匹配与记录，与 OpenSSH 行为一致。

### 5.3 指令执行

- 顺序执行配置中的每条指令，同一连接内**每条指令开启一个新的 session channel**（`channel_open_session` → `exec`），便于隔离单条指令的退出码。
- 每条指令的输出**流式转发**（不缓冲在内存）：stdout（`ChannelMsg::Data`）写本地 stdout，stderr（`ChannelMsg::ExtendedData` 且 `ext == 1`）写本地 stderr；退出码取自 `ChannelMsg::ExitStatus`。
- 单条指令失败（退出码非零）或执行出错，**不中断**后续指令执行。
- 远端未返回退出状态时，退出码记为"未提供"，不回填 0。
- 单条指令默认超时 300 秒（可用环境变量 `RTS_COMMAND_TIMEOUT` 覆盖，单位秒）；超时按该条失败处理并继续下一条。
- 收到退出码后，最多再等 2 秒让远端关闭通道；远端不关闭时收尾该条指令，不无限等待。
- 本地 stdout 被下游管道关闭（如 `| head`）时静默退出，不 panic。

### 5.4 结果回显格式

对第 i 条指令（共 n 条）：

```text
>>> [i/n] <原始指令>
<远端 stdout 原样输出>
<远端 stderr 原样输出，写到 stderr>
<<< exit=<退出码>
```

- stdout 数据原样写本地 stdout；stderr 数据原样写本地 stderr（保持流语义）。
- 头部 `>>>` 与尾部 `<<<` 写 stdout。
- 执行出错（无法开启通道等）时：`<<< error: <原因>`。
- 全部结束后输出汇总行：

```text
执行完成: 共 n 条指令, 成功 s 条, 失败 f 条
```

### 5.5 退出码

| 码 | 含义 |
|----|----|
| 0 | 全部指令执行完成且均成功 |
| 1 | 参数/配置文件错误（用法错、文件不可读、格式错误、缺行等） |
| 2 | 连接失败（网络不可达、超时、握手失败） |
| 3 | 认证失败 |
| 4 | 主机密钥校验失败（不匹配） |
| 5 | 指令执行过程中发生致命错误（连接中断等）——已完成的指令结果仍然回显 |

> 待评审确认：若存在指令退出码非零，是否让工具整体退出码非零（见 §6 冲突澄清 C2）。

## 6. 与既有约束的冲突 / 澄清

| 编号 | 事项 | 处置 |
|----|----|----|
| C1 | 协议行 `sftp` 的语义：SFTP 子系统不能执行指令 | 两种取值都走 SSH 传输层 + `exec` 通道执行指令；`sftp` 仅作配置标识保留，后续传文件可在同一连接上开 SFTP 子系统（已确认） |
| C2 | 单条指令退出码非零时的整体退出码 | 方案 A（推荐）：工具整体退出码仍为 0，失败明细在回显与汇总行体现，便于批量脚本不因单条失败中断；方案 B：整体退出码改为 5。**待确认** |
| C3 | `base/rts-core` 的 `logger` 目前是空壳（`logger/log.rs` 为空文件） | 本次补齐 `rts-core::logger::init()`：基于已有的 `ftail` + `log` 依赖初始化全局 logger（console 通道，级别由 `RTS_LOG_LEVEL` 环境变量控制，默认 `Warn`）。r_connector 通过 `rts-core` 使用日志 |
| C4 | 日志与回显同处 stdout 会互相污染 | ftail console 通道默认级别 `Warn`，且本工具的 `Info` 级日志（连接成功、逐条指令开始等）仅进日志；回显内容仍走 stdout |

## 7. 依赖清单

| crate | 版本 | 用途 | 备注 |
|-------|------|------|------|
| `russh` | 0.64.1 | SSH 客户端（连接、认证、exec 通道、known_hosts） | `default-features = false`，`features = ["flate2", "ring"]`；关掉默认的 `aws-lc-rs`（需 cmake/编译 C 代码）与 `rsa`（本次不用私钥） |
| `tokio` | 1.x | 异步运行时 | features: `rt-multi-thread`, `macros`, `net`, `io-util`, `time` |
| `log` | 0.4 | 日志门面 | rts-core 已用 |
| `rpassword` | 7.x | 密码交互输入（不回显） | 仅在配置密码为空时使用 |
| `rts-core` | 路径依赖 | 日志初始化 | workspace 内 |
| `rand`（dev） | 0.10 | 仅测试与 mock 服务端生成主机密钥 | 已在依赖树中（russh 依赖） |

`rpassword` 依赖 `libc`（unix 下的 termios），是纯 Rust crate，无需系统额外安装库。

### 7.1 环境变量

| 变量 | 作用 | 默认值 |
|----|----|-------|
| `RTS_LOG_LEVEL` | 日志级别（`off`/`error`/`warn`/`info`/`debug`/`trace`） | `warn` |
| `RTS_KNOWN_HOSTS` | known_hosts 文件路径 | `~/.ssh/known_hosts` |
| `RTS_COMMAND_TIMEOUT` | 单条指令超时秒数 | `300` |

## 8. 验证方式

### 8.1 单元测试（`cargo test`，无需远端）

`config.rs` 解析用例：

1. 规范四行配置 → 各字段正确。
2. 协议大小写不敏感（`SFTP`/`Ssh`）。
3. 端口缺省 → 22；`host:2222` → 2222；`[::1]:2222` → 正确切分；非法端口 → 报错。
4. 账号行：`user,` → 密码为空；`user,pa,ss` → 密码 `pa,ss`；无逗号 → 报错；用户名为空 → 报错。
5. 空行与 `#` 注释被跳过；指令行不被标签前缀处理影响。
6. 头部三行带 `协议:` / `连接:` / `账号:` 前缀时正确剥除。
7. 有效行不足 3 → 报错。
8. 非法协议值 → 报错。
9. 空指令列表 → 合法（只连接不执行）。

`cli.rs`：参数个数校验（0 个 / 2 个 / `--help` / `--version`）。

`hostkey.rs`：在临时目录构造 known_hosts 文件，验证"首次记录 → 命中 → 变更被拒"三条路径（`russh::keys` 提供了 path 变体接口，用临时文件驱动）。

### 8.2 手工验证

用本机 `sshd`（或容器内 openssh-server）作为远端：

1. 写一份配置文件指向本地 sshd（端口、账号密码）。
2. 跑 `r_connector conf.txt`，确认：连接提示 → 逐条指令回显 → 退出码行 → 汇总行 → 进程退出码符合预期。
3. 复跑一次，确认第二次不再出现"首次信任"提示（走 known_hosts 命中路径）。
4. 手工把 known_hosts 里的 key 改掉，确认拒绝连接并提示"主机密钥已变更"。
5. 用错误密码确认认证失败文案与退出码 3。
6. 密码留空确认交互式不回显输入。

### 8.3 无 sshd 时的替代验证（本次实际采用）

本机无 sshd（WSL 环境且非 root），因此提供 `toolkit/r_connector/examples/mock_ssh_server.rs`：
一个基于 `russh` 服务端 API 的 mock SSH 服务端，口令固定 `secret`，收到 exec 请求后按固定规则回显
（stdout 回 `mock: <指令>`；指令以 `!` 开头则回 stderr 并返回退出码 3），**不执行任何本地命令**。

```text
cargo run --example mock_ssh_server -- 127.0.0.1:22222     # 终端 1
RTS_KNOWN_HOSTS=/tmp/kh r_connector conf.txt                 # 终端 2
```

该方式覆盖：配置解析 → 连接 → 主机密钥 TOFU → 密码认证 → exec 通道 → stdout/stderr/退出码回显 → 汇总行 → 退出码。

此外，`session.rs` 内含基于同一 mock 服务端的进程内集成测试（`cargo test` 自动运行），无需手工干预。

## 9. 不做（重申）

- 不实现 SFTP 文件传输（仅保留协议标识与后续扩展位）。
- 不实现交互式 REPL、不实现私钥/证书/键盘交互认证。
- 不实现多主机并发与批量编排、不做本地管道串联。
- 不修改 rts-center。
