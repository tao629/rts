# db_caller.sh 验证记录

> 需求：新增 `scripts/db_caller.sh`，从 `.env` 读取 `DATABASE_URL`，连接数据库并执行传入的 SQL 文件。

## 环境

- WSL2 Ubuntu / bash 5.3.9
- 本机**未安装** `psql` / `mysql` / `sqlite3`，仓库 `.env` 里的 `DATABASE_URL` 是占位连接串（`<host>:5432`），
  没有可连的真实库，因此**无法做真实连库冒烟**。下列用例用「桩客户端 + 假 env 文件」验证脚本行为。

## 用例与实测结果

桩客户端放在 `/tmp/dbcaller_t1/bin`（`psql`/`mysql`/`sqlite3` 打印参数、`PGPASSWORD`/`MYSQL_PWD` 与 stdin），
假 env 文件 `/tmp/dbcaller_t1/env.*`。

| # | 场景 | 命令 | 实测结果 |
|---|------|------|----------|
| 1 | 语法检查 | `bash -n scripts/db_caller.sh` | 通过 |
| 2 | 帮助 | `scripts/db_caller.sh --help` | 打印用法，含默认值路径 |
| 3 | postgres 解析（口令 `p%40ss%3Aword`） | `db_caller.sh --env-file env.test --dry-run q.sql` | `[dry-run] psql --no-psqlrc -v ON_ERROR_STOP=1 -f q.sql postgresql://app@127.0.0.1:5432/rtsdb?sslmode=disable`（无口令） |
| 4 | 口令确实传给客户端 | 同上，实际执行 | `[stub psql] PGPASSWORD=p@ss:word`（`%40`→`@`、`%3A`→`:` 解码正确） |
| 5 | 口令含**未编码**的 `@` 与 `:` | `DATABASE_URL=postgresql://app:p@ss:word@127.0.0.1:5432/rtsdb` | `PGPASSWORD=p@ss:word`（按最后一个 `@` 切分） |
| 6 | 无主机（本地 socket） | `DATABASE_URL=postgresql:///rtsdb` | `psql ... postgresql:///rtsdb`，`PGPASSWORD` 不设置 |
| 7 | mysql | `DATABASE_URL=mysql://root:s3cr3t@127.0.0.1:3306/rtsdb` | `mysql --default-character-set=utf8mb4 --show-warnings --host=127.0.0.1 --port=3306 --user=root --database=rtsdb`，`MYSQL_PWD=s3cr3t`，SQL 经 stdin 传入 |
| 8 | sqlite | `DATABASE_URL=sqlite:///tmp/dbcaller_t1/test.db` | `sqlite3 -bail /tmp/dbcaller_t1/test.db`，SQL 经 stdin 传入 |
| 9 | 标准输入模式 | `cat q.sql \| db_caller.sh --env-file env.sqlite -` | SQL 从 stdin 读入并执行 |
| 10 | 多文件顺序执行 | `db_caller.sh --env-file env.test q.sql q2.sql` | 两个文件按顺序各执行一次 |
| 11 | 交互确认（pty） | `script -qec "db_caller.sh ... q.sql" /dev/null <<< "n"` | 打印目标库/来源/文件清单，输 `n` → `错误: 已取消`；输 `y` → 正常执行 |
| 12 | `--yes` 跳过确认 | `db_caller.sh --yes ...` | 直接执行，无提示 |
| 13 | 来源优先级 | `--url` > 进程环境变量 > 环境文件 | `--url 'postgresql://u:p@h:5432/db'` 与 `DATABASE_URL=...` 各自生效，环境文件被跳过 |
| 14 | 缺参数 | `db_caller.sh` | 打印用法 + `错误: 缺少 SQL 文件参数`（退出码 1） |
| 15 | 文件不存在 | `db_caller.sh no.sql` | `错误: SQL 文件不存在: no.sql` |
| 16 | env 文件不存在 | `db_caller.sh --env-file /tmp/nope.env q.sql` | `错误: 找不到环境文件 ...` |
| 17 | env 里无 DATABASE_URL | `错误: 环境文件 env.empty 中没有 DATABASE_URL` | 符合预期 |
| 18 | 不支持的 scheme | `DATABASE_URL=redis://...` | `错误: 暂不支持的数据库类型: redis（支持 postgresql/postgres、mysql/mariadb、sqlite）` |
| 19 | 客户端缺失 | 无桩客户端时执行 | `错误: 未找到 psql 客户端，请先安装 postgresql-client` |
| 20 | 未知选项 | `db_caller.sh --bogus q.sql` | `错误: 未知选项: --bogus（用 --help 查看用法）` |

## 已知限制

- 本机无 `psql`/`mysql`/`sqlite3`，且 `.env` 中是占位连接串，**未做真实连库验证**；
  上表验证的是「连接串解析 + 命令组装 + 口令传递 + 交互确认 + 错误处理」这一层。
- `mysql`/`sqlite3` 客户端靠 stdin 收 SQL，因此脚本对这两个库**不支持** SQL 文件里的 `\i` 客户端元命令
  （`psql` 走 `-f`，不受影响）。
- `sqlite` 连接串按「路径原样使用」处理，不做 `%XX` 解码（文件名可能是字面量）。
- 口令最终通过 `PGPASSWORD`/`MYSQL_PWD` 环境变量传给客户端，同主机其它用户理论上可从
  `/proc/<pid>/environ` 读到；若威胁模型要求更强隔离，可改用 `.pgpass` / `mysql_config_editor`。

## 结论

脚本行为符合预期，可交付。真实连库验证待有可用客户端与真实 `DATABASE_URL` 时补做。
