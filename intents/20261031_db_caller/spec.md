# 需求规格说明书（Spec）: db_caller.py —— PostgreSQL SQL 脚本执行工具

依据：`intents/20261031_db_caller/intent.md`（状态 finalize）。本文件是「设计阶段」输出物，由发起人评审后定稿。

## 1. 定位与边界

| 项 | 内容 |
|----|------|
| 名称 | `db_caller.py` |
| 位置 | `scripts/database/`（独立 uv 工程，与 cargo workspace 无耦合） |
| 运行时 | `uv run db_caller.py …`（uv 管理 Python 与依赖） |
| 目标库 | PostgreSQL（`DATABASE_URL` 必须为 `postgresql://` / `postgres://`） |
| 驱动 | psycopg 3（`psycopg[binary]`） |
| 输入 | 运行时参数传入的 SQL 脚本文件（DDL/DML/查询） |
| 不做 | 多数据库适配、图形化界面、SQL 方言转换、跨脚本原子事务、自动修复失败的迁移、字节级备份还原保证 |

## 2. 命令行接口

```
uv run db_caller.py [选项] <sql脚本> [<sql脚本> ...]
```

### 2.1 参数

| 参数 | 默认 | 说明 |
|------|------|------|
| `<sql脚本>` | 必填 | 一个或多个 SQL 文件路径，按给定顺序执行；每个文件是**独立**事务与独立幂等指纹 |
| `--env-file PATH` | 向上查找 `.env` | 指定环境变量文件；默认从脚本所在目录逐级向上找 `.env`（命中仓库根的 `.env`） |
| `--url URL` | 空 | 直接给连接串，覆盖 `DATABASE_URL`（仅应急；日志中脱敏，提示勿写入 shell 历史） |
| `--dry-run` | 关 | 只做切分与打印，不连库、不执行；输出每条语句的序号、起始行号、首行摘要 |
| `--force` | 关 | 忽略幂等记录，强制执行（同名脚本内容被改动过也照跑） |
| `--list-applied` | 关 | 列出迁移记录表里的已执行脚本后退出（不执行脚本） |
| `--no-backup` | 关 | 关闭执行前结构备份 |
| `--backup-dir DIR` | `scripts/database/backup` | 备份输出目录，不存在时自动创建 |
| `--migrations-schema NAME` | `public` | 迁移记录表所在 schema |
| `--max-rows N` | `100` | 查询结果集最多回显行数（超出提示截断） |
| `--statement-timeout SECONDS` | `0`（不限） | 会话级 `statement_timeout`，0 表示不限制 |
| `--connect-timeout SECONDS` | `10` | 建连超时 |
| `--quiet` | 关 | 只输出错误与最终摘要 |
| `--no-color` | 关 | 关闭 ANSI 颜色（非 TTY 时自动关闭） |
| `--verbose` | 关 | 打印每条语句的完整 SQL 文本 |

### 2.2 退出码

| 码 | 含义 |
|----|------|
| 0 | 成功（含「已执行过，跳过」） |
| 1 | 脚本执行失败（事务已整体回滚） |
| 2 | 参数/环境错误：文件不存在、缺 `DATABASE_URL`、URL scheme 不支持、脚本为空 |
| 3 | 数据库连接失败 |
| 4 | 备份失败（备份模式下备份写不成功则中止执行，不改库） |

## 3. 环境变量与凭据处理

- 连接串来自环境变量 `DATABASE_URL`；由 `--env-file` 指定的 `.env` 或向上查找到的 `.env` 载入进程环境（**不覆盖**已存在的进程环境变量）。
- 支持的 scheme：`postgresql`、`postgres`。其它 scheme → 退出码 2 并提示「本工具仅支持 PostgreSQL」。
- **脱敏**：任何日志/报错里的连接串经 `redact_url()` 处理，形如 `postgresql://***@host:port/dbname`；口令永不出现在输出中。
- `--url` 传入时，日志只打印脱敏形式。
- 仓库 `.env` 已被 gitignore；新增根 `.env.example` 给出 `DATABASE_URL` 占位写法，不含真实口令。

## 4. SQL 脚本切分

自研切分器（不依赖 sqlparse，保证 dollar-quoted / 嵌套注释 / 行号精度的确定性）：

- 状态机跟踪：普通文本、单引号字符串（`''` 转义）、双引号标识符（`""` 转义）、`E'...'` 转义字符串、行注释 `--`、块注释 `/* */`（**支持嵌套**）、dollar-quoted `$tag$ … $tag$`（`$tag$` 中 `tag` 为空或标识符）。
- 仅在「普通文本」状态下遇到 `;` 才结束一条语句；语句内保留原文，注释原样保留（便于 `--verbose` 回显）。
- 每条语句记录**脚本内起始行号**（1-based），用于报错定位。
- 文件末尾无分号时，最后一段非空内容也算一条语句。
- 空脚本（只有注释/空白）→ 退出码 2，提示「脚本为空」。
- 切分结果为 `list[Statement]`，`Statement` 含 `index`（1-based）、`sql`、`start_line`、`summary`（首行去掉注释后截断至 120 字符）。

## 5. 执行语义

- **单事务**：一个脚本文件内的所有语句在同一个事务中执行；任一条失败 → 回滚整个文件 → 该文件退出码 1，不再执行后续文件（除非 `--force`? 否：`--force` 只影响幂等跳过，不影响失败即停）。
- 事务内 DDL 依赖 PostgreSQL 的事务性 DDL（`CREATE TABLE` / `ALTER TABLE` / `DROP TABLE` 均可回滚）。
- 每条语句执行后打印：序号、起始行号、耗时（ms）、受影响行数（`rowcount`）。
- 结果集语句（`SELECT`/`RETURNING`/`WITH ... SELECT`/`SHOW` 等有返回的语句）以对齐表格回显，最多 `--max-rows` 行；超出时提示「已截断」。
- 失败信息包含：脚本路径、语句序号、起始行号、语句摘要、PG 错误码、主错误信息、detail/hint（若有）。
- 执行前打印「受影响表清单」（由脚本中 `CREATE TABLE` / `ALTER TABLE` / `DROP TABLE` 提取，含 schema 限定与引号标识符），供人工确认。

## 6. 幂等（迁移记录）

- 记录表：`schema_migrations`（位于 `--migrations-schema` 指定 schema，默认 `public`）；不存在时**自动创建**。
- 列：`id serial primary key`、`script_name text not null`、`checksum char(64) not null`、`applied_at timestamptz not null default now()`、`execution_ms integer`、`statement_count integer`；`checksum` 唯一索引。
- 指纹 = 脚本文件内容 SHA-256（十六进制小写）。
- 判定：
  - 同名 + 同 checksum 已记录 → **跳过**该脚本，打印「已执行过，跳过」，退出码 0；
  - 同名 + 不同 checksum → **报错**（脚本内容已改动），提示用 `--force` 强制执行，退出码 1；
  - 无记录 → 正常执行，成功后写入记录（与 DDL 同一事务提交，保证「改了库但没记账」不会发生）。
- `--list-applied`：按 applied_at 升序列出脚本名、checksum 前 12 位、应用时间、耗时、语句数。

## 7. 执行前备份

- 触发条件：默认开启（`--no-backup` 关闭）；备份对象 = 脚本涉及的表中**已存在**的那些（新建表无需备份）。
- 优先使用 `pg_dump`（若在 PATH 中）：
  - 通过 `PGHOST/PGPORT/PGUSER/PGPASSWORD/PGDATABASE` 环境变量传连接参数，**不把口令放进命令行参数**（避免出现在 `ps` 输出里）；
  - 参数：`--schema-only --no-owner --no-privileges --table=<表>`；
  - 表名逐个导出后合并为一个备份文件。
- `pg_dump` 不可用时，用 `pg_catalog` 内省重建结构，生成等价语句：列（名称、类型、可空、默认值）、主键、唯一约束、外键、CHECK 约束、索引。
  - **已知限制**：重建结果是「功能等价的结构」，非原始 DDL 文本；备份文件头部写入醒目提示说明该限制。
- 备份文件名：`<脚本名>_<YYYYmmdd_HHMMSS>.sql`，写入 `--backup-dir`；同目录生成 `.manifest.json`，记录来源脚本、SHA-256、受影响表清单、备份方式（pg_dump / 内省重建）、生成时间。
- 备份目录加入 `.gitignore`（可能含生产表结构）。
- 备份失败 → 退出码 4，**不执行脚本**（避免「改库了但没备份」）。

## 8. 输出规范

- 人类可读日志：阶段标题 + 缩进明细；示例：

```
▶ 脚本 sql/001_init.sql （sha256 3f2a…c81d，3 条语句）
  受影响表：public.orders、public.customers
  [1] 行 1  CREATE TABLE orders (…)                       12ms  影响 0 行
  [2] 行 14 ALTER TABLE orders ADD COLUMN note text        3ms   影响 0 行
  [3] 行 20 SELECT count(*) FROM orders                     2ms   2 行
      count
      -----
      2
  ✔ 提交成功（总耗时 21ms）
```

- 颜色：TTY 且未 `--no-color` 时使用（绿=成功，红=错误，黄=警告）；非 TTY 自动关闭（便于 CI 日志）。
- `--quiet`：只输出错误与结尾一行摘要。

## 9. 工程结构

```
scripts/database/
├── pyproject.toml      # uv 工程：requires-python>=3.11，依赖 psycopg[binary]、python-dotenv；dev 依赖 pytest
├── uv.lock             # uv lock 生成
├── README.md           # 用法、参数、退出码、备份限制说明
├── db_caller.py        # CLI 入口（argparse 解析 + 编排）
├── dbtool/
│   ├── __init__.py
│   ├── config.py       # .env 加载、连接串读取与脱敏
│   ├── splitter.py     # SQL 切分状态机
│   ├── tables.py       # 受影响表提取
│   ├── executor.py     # 连接、执行、回显、事务
│   ├── backup.py       # pg_dump / pg_catalog 内省备份
│   └── migrations.py   # schema_migrations 表与幂等判定
├── sql/                # 示例脚本
│   └── 001_example.sql
├── tests/              # pytest 单测（切分、脱敏、表名提取、指纹）
└── backup/             # 备份输出（gitignore）
```

根目录 `.gitignore` 追加：`scripts/database/.venv/`、`scripts/database/backup/`、`scripts/database/.pytest_cache/`、`scripts/database/**/__pycache__/`。
根目录新增 `.env.example`，示例 `DATABASE_URL=postgresql://user:password@host:5432/rts`。

## 10. 验证方式

允许在真实 `rts` 库建**临时 schema**（如 `db_caller_it_<pid>`）验证，用完 `DROP SCHEMA … CASCADE` 清理；不动 `public` 下现有业务表。

必须实测覆盖：

1. `--dry-run`：输出语句清单，不连库（用不存在的 URL 也能跑通）。
2. 建表 → 加字段 → 加约束 → 查回显，全链路在临时 schema 内生效。
3. 中途失败：脚本第 2 条故意写错 → 退出码 1，且第 1 条的建表被回滚（表不存在）。
4. 幂等：同脚本再跑 → 跳过；改内容再跑 → 报错；`--force` → 强制执行。
5. 备份：执行前生成 `.sql` 备份 + `.manifest.json`；`--no-backup` 不生成。
6. 脱敏：日志全文搜索不含明文口令。
7. 单元测试：pytest 全绿。

命令记录写入 `intents/20261031_db_caller/verify.md`。

## 11. 与现有约束的冲突检查

| 现有约定 | 冲突？ | 处理 |
|----------|--------|------|
| 凭据不入库 | 否 | 只读环境变量；口令不进代码/日志/命令行参数 |
| 注释与文档用中文 | 否 | 代码注释、README、日志均为中文 |
| 禁止在主仓开发 | 否 | 改动在 `.claude/worktrees/db-caller/` 的 `feature/db-caller` 分支 |
| `.env` 已 gitignore | 否 | 工具只读不改 |
| Rust 模块规范 | 否 | 本工具独立，不入 cargo workspace |

无冲突项需要裁剪。

## 12. 待发起人确认的点

1. 自研切分器（不引入 sqlparse）是否接受？——为拿到精确行号与 dollar-quoted 支持。
2. 备份默认开启、失败即中止（退出码 4）是否接受？
3. 迁移记录表默认落在 `public` schema 是否接受（生产 `public` 只读时用 `--migrations-schema` 切换）？
4. 验证用临时 schema，不动现有业务表，是否接受？
