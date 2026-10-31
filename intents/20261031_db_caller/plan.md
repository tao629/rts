# 开发计划（Plan）: db_caller.py —— PostgreSQL SQL 脚本执行工具

依据：`intent.md`（finalize）→ `spec.md`（已评审通过）。本文件是「构建阶段」输出物。

## 0. 实施原则

- 主仓 `main` 只读；全部改动在 worktree `.claude/worktrees/db-caller/`、分支 `feature/db-caller` 上完成。
- 交付链路：实现 → 自测（pytest）→ 真实库临时 schema 冒烟 → 写 `verify.md` → 提交 → 推分支 → 建 PR（无 gh CLI，走 GitHub REST API）→ 等用户显式确认后再合入 main。

## 1. 文件改动清单

| # | 文件 | 动作 | 说明 |
|---|------|------|------|
| 1 | `scripts/database/pyproject.toml` | 新增 | uv 工程；`requires-python = ">=3.11"`；deps: `psycopg[binary]>=3.2`、`python-dotenv>=1.0`；dev-deps: `pytest>=8`；`[tool.pytest.ini_options] pythonpath = ["."]` |
| 2 | `scripts/database/uv.lock` | 新增 | `uv lock` 生成并提交 |
| 3 | `scripts/database/dbtool/__init__.py` | 新增 | 包声明（中文模块说明） |
| 4 | `scripts/database/dbtool/config.py` | 新增 | `.env` 向上查找与载入、`DATABASE_URL` 读取、scheme 校验、`redact_url()` 脱敏 |
| 5 | `scripts/database/dbtool/splitter.py` | 新增 | SQL 切分状态机 + `Statement` 数据结构 + 每条语句起始行号 |
| 6 | `scripts/database/dbtool/tables.py` | 新增 | 从脚本提取受影响表（`CREATE/ALTER/DROP TABLE`，支持 schema 限定与引号标识符） |
| 7 | `scripts/database/dbtool/migrations.py` | 新增 | `schema_migrations` 自动建表、指纹判定、`--list-applied` 列表 |
| 8 | `scripts/database/dbtool/backup.py` | 新增 | `pg_dump` 优先 + `pg_catalog` 内省重建备份；manifest 生成 |
| 9 | `scripts/database/dbtool/executor.py` | 新增 | 连接、事务、逐条执行、结果集表格回显、失败定位 |
| 10 | `scripts/database/dbtool/output.py` | 新增 | 颜色、日志格式、表格渲染（避免与 executor 混在一起） |
| 11 | `scripts/database/db_caller.py` | 新增 | CLI 入口：`argparse` 参数、编排流程、退出码 |
| 12 | `scripts/database/sql/001_example.sql` | 新增 | 示例脚本（建表 + 加字段 + 查询） |
| 13 | `scripts/database/tests/test_splitter.py` | 新增 | 切分器单测（注释、字符串、dollar-quoted、嵌套注释、行号） |
| 14 | `scripts/database/tests/test_config.py` | 新增 | 脱敏、env 查找单测 |
| 15 | `scripts/database/tests/test_tables.py` | 新增 | 表名提取单测 |
| 16 | `scripts/database/README.md` | 新增 | 用法、参数表、退出码、备份限制、迁移记录说明 |
| 17 | `.gitignore` | 修改 | 追加 `scripts/database/.venv/`、`backup/`、`.pytest_cache/`、`__pycache__/` |
| 18 | `.env.example` | 新增 | 根目录示例配置（`DATABASE_URL` 占位） |
| 19 | `intents/20261031_db_caller/verify.md` | 新增 | 验证记录（命令 + 实测输出 + 结论） |

不改动：任何 Rust crate、`web-ui`、`.env`、现有 `rts` 库 `public` schema。

## 2. 实施顺序

1. worktree + 分支；`uv init` 骨架 + `uv lock`（确认 Python 3.14 + psycopg 3.3.x 可用，已实测可装）。
2. `config.py` → `splitter.py` → `tables.py`（纯逻辑，先写单测）。
3. `migrations.py` → `backup.py` → `executor.py` → `output.py`。
4. `db_caller.py` CLI 串起来。
5. pytest 全绿。
6. 真实库冒烟：临时 schema `db_caller_it_<pid>` 跑 spec §10 的 7 项。
7. `README.md`、`.gitignore`、`.env.example`、样例 SQL。
8. `verify.md` 记录实测结果 → 提交 → 推分支 → 建 PR。

## 3. 关键实现要点（易错点）

| 点 | 做法 |
|----|------|
| 切分器状态机 | 状态顺序：普通 → 行注释 → 块注释（可嵌套，用深度计数）→ 单引号 → 双引号 → E'' → dollar-quoted。只有「普通」状态遇 `;` 才断句 |
| dollar-quoted 标签 | `$` 后允许空或 `[A-Za-z_][A-Za-z0-9_]*`，再跟 `$`；结束标签必须与起始完全一致 |
| 行号 | 记录每条语句第一个非空白、非注释字符所在行；切分时逐字符累计换行 |
| 口令泄露 | 任何异常/日志经 `redact_url()`；`pg_dump` 用 `PGPASSWORD` 环境变量传口令，不进 argv |
| 失败定位 | 捕获 `psycopg.Error` → 取 `sqlstate`、`diag.message_primary`、`diag.message_detail`、`diag.message_hint` |
| 事务边界 | 每个文件一个 `conn.transaction()`；迁移记录写入与 DDL 同一事务 |
| 幂等判定 | 同名不同 checksum → 报错退出（除非 `--force`）；同 checksum → 跳过 |
| 备份失败 | 抛出专用异常 → 退出码 4，不进入执行阶段 |
| 内省重建 | 查 `information_schema.columns` + `pg_constraint` + `pg_indexes`，生成 `CREATE TABLE`/`ALTER TABLE ADD CONSTRAINT`/`CREATE INDEX` |
| 结果集回显 | `cursor.description` 非空即视为有结果集；`fetchmany(max_rows+1)` 判截断 |
| 退出码 | 0 成功/跳过；1 执行失败；2 参数环境错误；3 连接失败；4 备份失败 |

## 4. 风险与应对

| 风险 | 影响 | 应对 |
|------|------|------|
| 本机无 `pg_dump` | 备份走内省重建路径，结构非字节级还原 | spec 已声明限制；备份文件头部写明；真机验证该路径可用 |
| 内省重建漏掉索引/约束 | 回滚不完整 | 覆盖列/主键/唯一/外键/check/索引六类；manifest 记录方式；README 提示优先装 pg_dump |
| 真实库冒烟误伤业务表 | 生产数据受损 | 一律在临时 schema 内建表，用完 `DROP SCHEMA ... CASCADE`；脚本里表名显式带 schema 前缀 |
| `schema_migrations` 建在 `public` | 生产 `public` 只读则失败 | `--migrations-schema` 可切；失败信息给出行 2 提示 |
| psycopg 3 与 Python 3.14 兼容性 | 运行时报错 | 已实测 psycopg 3.3.6 可装可导入；冒烟阶段再确认 |
| 幂等表污染 | 同名脚本被误跳过 | checksum 不一致时报错而非跳过；`--force` 可覆盖 |
| 首次 `uv sync` 需联网 | 离线环境无法装依赖 | 已确认本机可访问 PyPI；`uv.lock` 提交以便复现 |
| 迁移记录表自身被脚本改动 | 幂等失效 | 脚本若 DROP `schema_migrations` 会导致后续判定失效 → 在脚本影响表清单里标注警告 |

## 5. 验证方式（与 spec §10 一致）

```bash
cd scripts/database
uv sync                                  # 装依赖
uv run pytest -q                        # 单元测试
uv run db_caller.py --dry-run sql/001_example.sql            # 预演，不连库
uv run db_caller.py sql/001_example.sql --migrations-schema <临时schema>   # 真实库冒烟
```

冒烟脚本清单（临时 schema 内建表 → 加字段 → 加约束 → 查询回显 → 幂等跳过 → 中途失败整单回滚 → 备份文件生成 → 清理 schema），逐条实测输出贴进 `verify.md`。

## 6. 交付

1. worktree 分支提交（中文 conventional 前缀，如 `feat(scripts): 新增 db_caller.py SQL 脚本执行工具`）。
2. `git push -u origin feature/db-caller`。
3. 走 GitHub REST API 建 PR（token 从 `~/.git-credentials`），描述末尾附 🤖 署名行。
4. **合并 main / 删分支前向用户显式确认**（本仓无 CI，权限拦截会拒绝）。
