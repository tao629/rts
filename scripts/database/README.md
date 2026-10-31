# db_caller —— PostgreSQL SQL 脚本执行工具

从 `.env` 读取 `DATABASE_URL`，连接 PostgreSQL，执行**运行时通过参数传入**的 SQL 脚本文件
（建表、加字段、改约束、灌数据、查询等）。

## 快速开始

```bash
cd scripts/database
uv sync                                  # 安装依赖（psycopg3 + python-dotenv）
uv run db_caller.py --dry-run sql/001_example.sql    # 预演：只切分打印，不连库
uv run db_caller.py sql/001_example.sql               # 执行
```

在**仓库根目录**调用时用 `--directory`（此时工作目录变成 `scripts/database`，脚本路径相对它写）：

```bash
uv run --directory scripts/database db_caller.py --dry-run sql/001_example.sql
```

连接串来自仓库根目录的 `.env`（工具会从自身所在目录逐级向上查找 `.env`）：

```dotenv
DATABASE_URL=postgresql://用户名:口令@主机:5432/库名
```

口令只存在于进程内存与环境变量里，**不会**出现在命令行、日志、备份文件里。

## 命令行参数

```
uv run db_caller.py [选项] <sql脚本> [<sql脚本> ...]
```

| 参数 | 默认 | 说明 |
|------|------|------|
| `<sql脚本>` | 必填 | 一个或多个 SQL 文件；按顺序执行，每个文件是**独立**事务 |
| `--env-file PATH` | 向上查找 `.env` | 指定环境文件 |
| `--url URL` | 空 | 直接给连接串（覆盖 `DATABASE_URL`）；注意不要写进 shell 历史 |
| `--dry-run` | 关 | 只切分并打印语句，不连库、不执行 |
| `--force` | 关 | 忽略幂等记录，强制执行 |
| `--list-applied` | 关 | 列出已执行的迁移记录后退出 |
| `--no-backup` | 关 | 关闭执行前的结构备份 |
| `--backup-dir DIR` | `scripts/database/backup` | 备份输出目录 |
| `--migrations-schema NAME` | `public` | 迁移记录表所在 schema |
| `--max-rows N` | `100` | 查询结果最多回显行数 |
| `--statement-timeout SECONDS` | `0`（不限） | 单条语句超时 |
| `--connect-timeout SECONDS` | `10` | 连接超时 |
| `--quiet` | 关 | 只输出错误与最终摘要 |
| `--no-color` | 关 | 关闭彩色输出（非 TTY 时自动关闭） |
| `--verbose` | 关 | 打印每条语句的完整 SQL |

### 退出码

| 码 | 含义 |
|----|------|
| 0 | 成功（含「已执行过，跳过」） |
| 1 | 脚本执行失败（事务已整体回滚） |
| 2 | 参数/环境错误（文件不存在、缺 `DATABASE_URL`、URL 不是 PostgreSQL、脚本为空） |
| 3 | 数据库连接失败 |
| 4 | 备份失败（已中止执行，数据库未改动） |

## 行为约定

### 单事务

一个脚本文件里的所有语句在**同一个事务**里执行，任一条失败即整体回滚，错误信息会给出
**失败语句的序号、脚本内行号、语句摘要与 PG 错误码（SQLSTATE）**：

```text
错误: 脚本执行失败：sql/002_alter.sql
错误:   失败语句：第 2 条，脚本第 3 行
错误:   语句摘要：ALTER TABLE db_caller_it.orders ADD CONSTRAINT ...
错误:   SQLSTATE：42710
错误:   错误信息：constraint "orders_name_not_blank" for relation "orders" already exists
错误:   事务已整体回滚，数据库未留下本次脚本的改动。
```

### 语句切分

自研状态机切分，正确处理：

- `--` 行注释、`/* … */` 块注释（**支持嵌套**）；
- 单引号字符串（含 `''` 与 `E'…'` 转义）、双引号标识符；
- `$$ … $$` 与 `$tag$ … $tag$` dollar-quoted 函数体（里面的分号不是分隔符）。

### 执行前备份

默认在执行前把脚本涉及的**已存在**表的结构备份到 `backup/`：

- 本机有 `pg_dump` 时用它做精确备份（口令经 `PGPASSWORD` 环境变量传递，不进命令行参数）；
- 没有 `pg_dump` 时用 `pg_catalog` 内省重建 `CREATE TABLE` / 约束 / 索引语句。
  **此时备份文件不是原始 DDL 文本**，文件头部会写明该限制，仅供人工参考。

同时生成 `*.manifest.json`，记录来源脚本、SHA-256、受影响表与备份方式。
备份失败会中止执行（退出码 4），避免「改了库却没备份」。

### 幂等

迁移记录表 `schema_migrations`（默认 `public` schema，缺表自动创建）记录每个已执行脚本的
「文件名 + 内容 SHA-256 指纹」：

- 同名同指纹 → 跳过（提示已执行过）；
- 同名但内容被改过 → 报错并提示加 `--force`；
- `--force` → 强制重跑（更新原记录，不新增行）。

迁移记录与脚本的 DDL 在同一事务里提交，不会出现「库改了但没记账」。

## 目录结构

```text
scripts/database/
├── db_caller.py        # CLI 入口
├── dbtool/             # 实现包
│   ├── config.py       # .env 查找、连接串读取与脱敏
│   ├── splitter.py     # SQL 切分状态机
│   ├── tables.py       # 受影响表提取
│   ├── migrations.py   # 迁移记录与幂等判定
│   ├── backup.py       # 执行前结构备份
│   ├── executor.py     # 事务、逐条执行、结果回显
│   └── output.py       # 日志着色与表格渲染
├── sql/                # 示例脚本
├── tests/              # pytest 单元测试
└── backup/             # 备份输出（gitignore）
```

## 开发

```bash
uv sync
uv run pytest -q          # 单元测试（不需要数据库）
uv run db_caller.py --help
```

## 已知限制

- 仅支持 PostgreSQL（连接串 scheme 必须是 `postgresql://` / `postgres://`）；
- 无 `pg_dump` 时的备份是「结构重建版」，不是原始 DDL 文本；
- 工具**不会**在执行前询问确认，脚本内容由人负责；生产库上建议先用 `--dry-run` 复核；
- 迁移记录表按「脚本文件名」识别脚本，重命名脚本会被当成新脚本。
