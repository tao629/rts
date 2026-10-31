# 验证记录（Verify）: db_caller.py

对应 `intent.md`（finalize）→ `spec.md`（已评审）→ `plan.md`。本文件记录**实测**的命令与结果。

> PR：#14 https://github.com/tao629/rts/pull/14 （分支 `feature/db-caller-python`）

## 环境

| 项 | 值 |
|----|----|
| 主机 | WSL2 Ubuntu（Linux 5.15+），bash 5.x |
| Python | CPython 3.14.10（uv 管理，`.venv`） |
| uv | 0.13.6 |
| 依赖 | psycopg 3.3.6、psycopg-binary 3.3.6、python-dotenv 1.2.4；dev: pytest 9.2.0 |
| 数据库 | PostgreSQL 18.6（Debian 18.6-1.pgdg13+2），库名与用户名见 `.env`（**主机、端口、用户名已脱敏，不入库**） |
| `pg_dump` | **未安装** → 备份走 `pg_catalog` 内省重建路径（本次即验证该路径） |

真实库 `rts` 的 `public` schema 原有 2 张表（`MIB_FILE`、`MIB_OBJECT`），验证前后未变动。
所有写操作都在临时 schema `db_caller_it` 内进行，验证结束已 `DROP SCHEMA ... CASCADE` 清理。

## 一、单元测试

```bash
cd scripts/database && uv run pytest -q
```

```text
.......................................                                  [100%]
39 passed in 0.04s
```

覆盖：SQL 切分（注释、嵌套块注释、字符串转义、dollar-quoted、行号、空脚本、未闭合报错）、
连接串解析与脱敏、`.env` 查找与来源优先级、受影响表提取、表格渲染与着色开关。

## 二、逐条对照 spec.md 验收标准

### 1. `--dry-run` 预演（不连库）

```bash
uv run db_caller.py --dry-run /tmp/dbcaller_smoke_20261031/001_init.sql
```

```text
▶ 预演 /tmp/dbcaller_smoke_20261031/001_init.sql （sha256 36300bf7b766，3 条语句）
  受影响表：db_caller_it.orders
  [1] 行 2  CREATE SCHEMA IF NOT EXISTS db_caller_it
  [2] 行 4  CREATE TABLE db_caller_it.orders (
  [3] 行 9  INSERT INTO db_caller_it.orders (name) VALUES ('甲'), ('乙')
  （预演模式：未连接数据库，未执行任何语句）
```

退出码 0，未建立数据库连接（预演路径在连接前返回）。

### 2. 建表 → 加字段 → 加约束 → 查询回显

```bash
uv run db_caller.py /tmp/dbcaller_smoke_20261031/001_init.sql /tmp/dbcaller_smoke_20261031/002_alter.sql \
  --env-file <仓库根>/.env --migrations-schema db_caller_it
```

```text
目标库：postgresql://<user>@<host>:<port>/rts（来源：环境文件 <仓库根>/.env）
▶ 执行 /tmp/dbcaller_smoke_20261031/001_init.sql （sha256 36300bf7b766，3 条语句，目标 postgresql://<user>@<host>:<port>/rts）
  受影响表：db_caller_it.orders
  无需备份（涉及的表尚不存在）
  [1] 行 2  CREATE SCHEMA IF NOT EXISTS db_caller_it    27ms  影响 — 行
  [2] 行 4  CREATE TABLE db_caller_it.orders (    34ms  影响 — 行
  [3] 行 9  INSERT INTO db_caller_it.orders (name) VALUES ('甲'), ('乙')    29ms  影响 2 行
✔ /tmp/dbcaller_smoke_20261031/001_init.sql 提交成功（3 条语句，总耗时 117ms）
▶ 执行 /tmp/dbcaller_smoke_20261031/002_alter.sql （sha256 23db46478d9a，3 条语句，目标 postgresql://<user>@<host>:<port>/rts）
  受影响表：db_caller_it.orders
  已备份表结构：.../backup/002_alter_20261031_102556.sql（pg_catalog 内省重建）
  [1] 行 2  ALTER TABLE db_caller_it.orders ADD COLUMN note text    25ms  影响 — 行
  [2] 行 3  ALTER TABLE db_caller_it.orders ADD CONSTRAINT orders_name_not_blank CHECK (name <> '')    26ms  影响 — 行
  [3] 行 4  SELECT id, name, note FROM db_caller_it.orders ORDER BY id    25ms  影响 2 行

id  name  note
--  ----  ----
1   甲     NULL
2   乙     NULL

✔ /tmp/dbcaller_smoke_20261031/002_alter.sql 提交成功（3 条语句，总耗时 127ms）
全部脚本执行完成
```

退出码 0。DDL 回显 `影响 — 行`（PG 对 DDL 不返回行数），`INSERT` 回显 `影响 2 行`，`SELECT` 结果以表格打印。

### 3. 中途失败 → 整单回滚

```bash
uv run db_caller.py /tmp/dbcaller_smoke_20261031/003_fail.sql --env-file … --migrations-schema db_caller_it
```

```text
错误: 脚本执行失败：/tmp/dbcaller_smoke_20261031/003_fail.sql
错误:   失败语句：第 2 条，脚本第 3 行
错误:   语句摘要：ALTER TABLE db_caller_it.should_not_exist ADD COLUMN x int NOT NULL DEFAULT '不是数字'
错误:   SQLSTATE：22P02
错误:   错误信息：invalid input syntax for type integer: "不是数字"
错误:   事务已整体回滚，数据库未留下本次脚本的改动。
```

退出码 **1**。随后直查数据库确认：

```text
should_not_exist 不存在: True          # 第 1 条 CREATE TABLE 已随事务回滚
迁移记录: ['001_init.sql', '002_alter.sql']   # 失败脚本未留下迁移记录
```

### 4. 幂等

```bash
# 4.1 重复执行 → 跳过
uv run db_caller.py …/002_alter.sql --migrations-schema db_caller_it
```

```text
▶ 跳过 /tmp/dbcaller_smoke_20261031/002_alter.sql （已执行过，sha256 23db46478d9a，记录于 2026-10-31T02:25:57.444944+00:00）
全部脚本执行完成
```

退出码 0，库内无任何改动。

```bash
# 4.2 同名但内容被改过 → 报错并提示 --force
uv run db_caller.py …/modified/002_alter.sql --migrations-schema db_caller_it
```

```text
错误: 脚本 002_alter.sql 已执行过，但内容与记录不一致（记录指纹 23db46478d9a，当前 abfefe497cf5）
错误:   如确认要重新执行，请加 --force。
```

退出码 **1**，未执行任何语句。

```bash
# 4.3 --force → 强制重跑，并更新原记录
uv run db_caller.py …/004_force.sql --migrations-schema db_caller_it --no-backup --force
```

```text
  [1] 行 1  CREATE TABLE IF NOT EXISTS db_caller_it.force_demo (id int)    26ms  影响 — 行
  [2] 行 2  INSERT INTO db_caller_it.force_demo (id) VALUES (1)    26ms  影响 1 行
✔ /tmp/dbcaller_smoke_20261031/004_force.sql 提交成功（2 条语句，总耗时 105ms）
```

直查确认：强制重跑后 `force_demo` 行数由 1 变 2，迁移记录仍是 3 条（UPSERT 更新而非新增）。

```bash
# 4.4 已执行清单
uv run db_caller.py --list-applied --env-file … --migrations-schema db_caller_it
```

```text
脚本             指纹            应用时间                              耗时     语句数
-------------  ------------  --------------------------------  ------  ---
001_init.sql   36300bf7b766  2026-10-31T02:25:25.337114+00:00  117ms  3
002_alter.sql  23db46478d9a  2026-10-31T02:25:57.444944+00:00  127ms  3
```

### 5. 执行前备份

备份文件 `backup/002_alter_20261031_102556.sql`（`pg_dump` 未安装，走内省重建）：

```sql
-- db_caller 表结构备份
-- 备份时间: 2026-10-31T10:25:56
-- 来源脚本: /tmp/dbcaller_smoke_20261031/002_alter.sql
-- 来源指纹: sha256:23db46478d9a78ab3ab1f709838ea16e25c746026be998f115ec84c87d846333
-- 受影响表: db_caller_it.orders
-- 备份方式: pg_catalog 内省重建
-- 注意: 本文件由 pg_catalog 内省重建，不是原始 DDL 文本，仅供人工参考。

CREATE TABLE db_caller_it.orders (
    id integer DEFAULT nextval('db_caller_it.orders_id_seq'::regclass) NOT NULL,
    name text NOT NULL
);
ALTER TABLE db_caller_it.orders ADD CONSTRAINT orders_pkey PRIMARY KEY (id);
```

同目录生成 `002_alter_20261031_102556.manifest.json`：

```json
{
  "generated_at": "2026-10-31T10:25:56",
  "source_script": "/tmp/dbcaller_smoke_20261031/002_alter.sql",
  "source_sha256": "23db46478d9a…333",
  "method": "pg_catalog 内省重建",
  "tables": ["db_caller_it.orders"],
  "backup_file": "002_alter_20261031_102556.sql"
}
```

`--no-backup` 时不生成备份文件（实测 `--no-backup` 运行后 `backup/` 目录未新增文件）。

### 6. 口令脱敏

从 `.env` 取出真实口令，对完整输出做全文匹配：

```text
口令长度: 10
✅ 日志中无明文口令
脱敏连接串出现次数: 1
```

日志里只出现 `postgresql://<user>@<host>:<port>/rts`。

### 7. 退出码与错误路径

| 场景 | 命令要点 | 实测输出 | 退出码 |
|------|----------|----------|--------|
| SQL 文件不存在 | `db_caller.py /tmp/nope.sql` | `错误: SQL 文件不存在：/tmp/nope.sql` | 2 |
| 脚本为空 | 空文件 | `错误: SQL 文件没有可执行语句：…/empty.sql` | 2 |
| 不支持的 scheme | `DATABASE_URL=mysql://…` | `错误: 暂不支持的数据库类型：mysql（本工具仅支持 PostgreSQL）` | 2 |
| 缺 `DATABASE_URL` | 空 env 文件 | `错误: 未找到 DATABASE_URL：请在 .env 中配置，或用 --env-file 指定环境文件，或用 --url 直接给出连接串` | 2 |
| 连接失败 | 端口 59999 | `错误: 连接数据库失败（进程环境变量 DATABASE_URL）：connection timeout expired` | 3 |
| 脚本执行失败 | 见上文第 3 项 | 语句级错误信息 | 1 |
| 正常执行 / 跳过 | — | — | 0 |

### 8. 清理

```text
清理后 public 表数: 2
临时 schema 是否残留: False
public 表: ['MIB_FILE', 'MIB_OBJECT']
```

验证用到的临时 schema `db_caller_it` 已删除，`public` schema 与验证前一致。

## 三、验证过程中发现并修复的问题

| # | 问题 | 现象 | 修复 |
|---|------|------|------|
| 1 | `SET statement_timeout = %s` 不支持参数占位符 | 连接即报 `syntax error at or near "$1"` | 改用 `SELECT set_config('statement_timeout', %s, false)` |
| 2 | 环境文件被载入 `os.environ`，来源提示失真 | 显示「进程环境变量 DATABASE_URL」，实际来自文件 | 不再写回 `os.environ`，改用 `dotenv_values` 直接读取 |
| 3 | 只读查询的隐式事务未提交，`conn.transaction()` 退化成 SAVEPOINT | 脚本显示「提交成功」但表未真正建出来（连接关闭时外层事务被回滚） | 执行前 `conn.commit()` 结束隐式事务，保证 `conn.transaction()` 是真正的事务 |
| 4 | `pg_get_expr(d.adbin, d.attrelid)` 列名写错 | 备份时报 `column d.attrelid does not exist` | 改为 `d.adrelid`（`pg_attrdef` 的实际列名） |
| 5 | 备份阶段数据库异常直接抛 traceback | 输出 Python 堆栈，退出码错误 | 备份期异常统一包成 `BackupError`，退出码 4 |
| 6 | `--force` 重跑时指纹唯一索引冲突 | `duplicate key value violates unique constraint "schema_migrations_checksum_idx"` | 迁移记录写入改为 `ON CONFLICT (checksum) DO UPDATE` |
| 7 | 显式 `--env-file` 不存在时，即使进程环境已有 URL 也直接报错 | 无法使用 | 改为：进程环境有 `DATABASE_URL` 时告警并继续 |

## 四、已知限制

1. 本机无 `pg_dump`，备份只验证了 `pg_catalog` 内省重建路径；`pg_dump` 路径（口令经
   `PGPASSWORD` 传递）代码已实现但**未实测**，装上 `postgresql-client` 后建议补测。
2. 内省重建的备份不是原始 DDL 文本（例如 `serial` 会被展开成 `integer DEFAULT nextval(...)`），
   仅作人工参考，不能直接当作还原脚本使用。
3. 工具执行前**不做交互确认**，与已删除的 shell 版 `scripts/db_caller.sh`（有 `-y/--yes` 确认）不同；
   生产库使用建议先 `--dry-run` 复核。
4. `--list-applied` / `--dry-run` 之外的路径都要求能连上真实库；离线环境只有 `--dry-run` 可用。
5. 迁移记录按「脚本文件名 + 内容指纹」识别脚本，重命名脚本会被当作新脚本执行。

## 五、结论

spec.md 的验收标准（§10 的 7 项）全部实测通过：预演不连库、DDL/DML/查询链路可用、
失败整单回滚、幂等跳过与冲突报错、`--force` 可重跑、执行前备份 + manifest、
日志无明文口令、单元测试全绿。验证用到的临时 schema 已清理，`rts` 库 `public` schema 未受影响。

可交付。PR：#14 https://github.com/tao629/rts/pull/14
