# 意图（Intent）: 用 uv + Python 开发数据库脚本执行工具 db_caller.py

Author: zhtao（仓库维护者）. 状态：finalize

## 问题（Problem）

仓库根目录 `.env` 已配置 `DATABASE_URL`（postgresql://…/rts），但目前没有任何命令行工具能拿它去执行 SQL 脚本。
每次要改表结构（建表、加字段、改约束），只能手工起 `psql` 粘贴语句，或临时写一次性脚本：

- 连接信息散落在各处，容易把口令写进代码、日志或命令历史，违反仓库「凭据不入库」的约定；
- 没有统一的「拆脚本 → 执行 → 出错定位 → 回滚」流程，一条语句失败后数据库停在中间态，人工收拾容易漏；
- DDL 没有留痕，重复执行同一个迁移脚本会二次报错，也不知道哪些改动已经落库。

## 预期结果（Proposed outcome）

在 `scripts/database/` 下交付一个独立的 uv 工程，主程序 `db_caller.py`，具备：

1. **连接**：从 `.env`（默认自动向上查找仓库根目录的 `.env`，可用 `--env-file` 覆盖）读取 `DATABASE_URL`，用 psycopg3 连接 PostgreSQL；口令只存在于进程环境变量里，不进代码、不进日志、不进命令行历史。
2. **执行**：运行时通过参数传入 SQL 脚本文件路径（如 `uv run db_caller.py sql/001_init.sql`），自动按语句切分（含 `--`/`/* */` 注释、`$$ ... $$` dollar-quoted 函数体），逐条执行并打印序号、耗时、受影响行数。
3. **事务**：默认整个脚本一个事务，任一语句失败即整体回滚，错误信息带**语句序号 + 脚本内起始行号**，便于定位。
4. **回显**：DML（INSERT/UPDATE/DELETE）打印受影响行数；SELECT/DML 返回结果集时以表格形式输出（可选 `--max-rows` 截断）。
5. **预演**：`--dry-run` 只做切分与打印，不连库、不执行，用来审阅脚本。
6. **备份**：执行前自动把脚本涉及的表（CREATE/ALTER/DROP 命中的表名）结构备份到带时间戳的 `.sql` 文件；`pg_dump` 可用时优先用它做精确备份，不可用时用 `pg_catalog` 内省重建 `CREATE TABLE`/约束/索引语句。
7. **幂等**：维护迁移记录表 `schema_migrations`（脚本指纹 = 文件内容 SHA-256 + 文件名），已执行过的脚本默认跳过；`--force` 可强制重跑，`--list-applied` 可查看已执行清单。
8. **可用性**：中文注释与文档（`README.md`），`uv run` 即可运行；错误码非 0 退出，便于 CI/脚本串联。

完成标识：在真实 rts 库上建一个临时测试 schema，验证「建表 → 加字段 → 改约束 → 查询回显 → 幂等跳过 → 故障整单回滚 → 备份文件生成」全链路通过，临时 schema 清理干净，现有 `rts` 公共 schema 不受影响。

## 受影响的模块（Affected modules）

| 对象 | 改动 |
|------|------|
| `scripts/database/pyproject.toml` | 新建：uv 工程定义，依赖 psycopg[binary]、sqlparse、python-dotenv |
| `scripts/database/uv.lock` | 新建：`uv lock` 生成 |
| `scripts/database/db_caller.py` | 新建：主程序（脚本切分、连接、执行、备份、迁移记录） |
| `scripts/database/README.md` | 新建：用法、参数、退出码说明 |
| `scripts/database/sql/` | 新建：示例 SQL 脚本（建表/加字段） |
| `scripts/database/tests/` | 新建：切分逻辑单元测试（pytest） |
| `scripts/database/backup/` | 新建：备份输出目录（gitignore） |
| `intents/20261031_db_caller/` | 新建：intent.md / spec.md / plan.md / verify.md |
| 仓库根 `.gitignore` | 修改：忽略 `scripts/database/.venv/`、`scripts/database/backup/` |
| 仓库根 `.env` | **不提交**（已被 gitignore）；必要时新增 `.env.example` 说明 `DATABASE_URL` 用途 |
| 仓库 `rts` 数据库 | 运行时新增 `schema_migrations` 迁移记录表；验证期临时 schema 用完即删 |

不改动任何 Rust crate 与 `web-ui`；本工具与 cargo workspace 无耦合。

## 约束条件（Constraints）

- **凭据不入库**：`DATABASE_URL` 只从环境变量读取；日志/报错里连接串必须脱敏（只显示 scheme、host、库名，不显示口令）；`--url` 覆盖参数一旦使用需提醒「不要写进 shell 历史」。
- **只读主仓**：所有代码改动在 `.claude/worktrees/db-caller/` 的 `feature/db-caller` 分支上做，主仓 `main` 不切分支、不暂存、不提交。
- **Python 版本**：用 uv 管理，工程 `requires-python = ">=3.11"`；本机 Python 3.14 + psycopg 3.3.6 已验证可装。
- **数据库范围**：只支持 PostgreSQL（URL 必须是 `postgresql://` / `postgres://`），其它 scheme 明确报错，不做多数据库适配。
- **破坏性动作**：DDL 脚本由人写、人传参，工具只负责忠实执行 + 留痕；不内置 `DROP DATABASE` 之类的额外危险能力。
- **备份不保证字节级还原**：`pg_dump` 不在时用内省重建，备份文件是「结构重建版」，文档中写明该限制。
- **兼容性**：仅在 Linux/WSL 下验证；Windows 下 uv 同样可用，但命令写法以 `uv run` 为准。

## 未解决的问题（Open questions）

> 2026-10-31 评审已拍板：

1. **迁移记录表放在哪个 schema** → 自动建表，默认 `public`，提供 `--migrations-schema` 覆盖（生产 `public` 只读时指向 `rts_migrations`）。
2. **备份目录默认值** → 默认 `scripts/database/backup/`，生成物 gitignore；`--no-backup` 可关闭。
3. **是否需要 `--init` 引导命令** → 不需要，缺表自动创建。
4. **失败后的清理责任** → 工具不做自动修复；执行前自动打印「受影响表清单」作为日志的一部分，无需额外 `--print-plan` 开关。
