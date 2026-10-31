#!/usr/bin/env python3
"""db_caller.py —— 从 `.env` 读取 `DATABASE_URL`，连接 PostgreSQL 并执行 SQL 脚本文件。

用法：
    uv run db_caller.py [选项] <sql脚本> [<sql脚本> ...]

核心语义：

- 一个脚本文件 = 一个事务，任一语句失败则整体回滚；
- 执行前自动备份受影响表的结构（pg_dump 优先，回退 pg_catalog 内省重建）；
- 迁移记录表 `schema_migrations` 保证同一脚本不会被重复执行；
- 连接串里的口令只存在于进程内存与环境中，不出现在命令行、日志与备份文件里。
"""

from __future__ import annotations

import argparse
import hashlib
import os
import sys
import time
from pathlib import Path

import psycopg

if __package__ in (None, ""):  # 允许 `uv run db_caller.py` 直接运行
    sys.path.insert(0, str(Path(__file__).resolve().parent))

from dbtool import backup as backup_mod
from dbtool import config as config_mod
from dbtool import migrations as migrations_mod
from dbtool.executor import ScriptFailure, connect, execute_statements
from dbtool.output import Console, render_table
from dbtool.splitter import Statement, split_sql
from dbtool.tables import extract_tables, format_table_list

EXIT_OK = 0
EXIT_SCRIPT_FAILED = 1
EXIT_CONFIG = 2
EXIT_CONNECT = 3
EXIT_BACKUP = 4

DEFAULT_BACKUP_DIR = Path(__file__).resolve().parent / "backup"


def build_parser() -> argparse.ArgumentParser:
    """构造命令行解析器。"""
    parser = argparse.ArgumentParser(
        prog="db_caller.py",
        description="从 .env 读取 DATABASE_URL，连接 PostgreSQL 并执行运行时传入的 SQL 脚本文件。",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=(
            "示例:\n"
            "  uv run db_caller.py --dry-run sql/001_example.sql      # 预演，不连库\n"
            "  uv run db_caller.py sql/001_example.sql                 # 执行\n"
            "  uv run db_caller.py --list-applied                     # 查看已执行清单\n"
        ),
    )
    parser.add_argument("scripts", nargs="*", metavar="<sql脚本>", help="要执行的 SQL 脚本文件")
    parser.add_argument("--env-file", help="指定环境文件，默认从脚本目录向上查找 .env")
    parser.add_argument("--url", help="直接指定连接串（覆盖 DATABASE_URL），注意不要写进 shell 历史")
    parser.add_argument("--dry-run", action="store_true", help="只切分并打印语句，不连库、不执行")
    parser.add_argument("--force", action="store_true", help="忽略幂等记录，强制执行")
    parser.add_argument("--list-applied", action="store_true", help="列出已执行的迁移记录后退出")
    parser.add_argument("--no-backup", action="store_true", help="关闭执行前的结构备份")
    parser.add_argument(
        "--backup-dir",
        default=str(DEFAULT_BACKUP_DIR),
        help=f"备份输出目录，默认 {DEFAULT_BACKUP_DIR}",
    )
    parser.add_argument(
        "--migrations-schema", default="public", help="迁移记录表所在 schema，默认 public"
    )
    parser.add_argument("--max-rows", type=int, default=100, help="查询结果最多回显行数，默认 100")
    parser.add_argument(
        "--statement-timeout", type=int, default=0, help="单条语句超时秒数，0 表示不限"
    )
    parser.add_argument("--connect-timeout", type=int, default=10, help="连接超时秒数，默认 10")
    parser.add_argument("--quiet", action="store_true", help="只输出错误与最终摘要")
    parser.add_argument("--no-color", action="store_true", help="关闭彩色输出")
    parser.add_argument("--verbose", action="store_true", help="打印每条语句的完整 SQL")
    return parser


def load_script(path_text: str) -> tuple[Path, str, list[Statement]]:
    """读取脚本文件并切分，返回 `(路径, 内容, 语句列表)`。"""
    path = Path(path_text)
    if not path.is_file():
        raise config_mod.ConfigError(f"SQL 文件不存在：{path}")
    try:
        content = path.read_text(encoding="utf-8")
    except OSError as exc:
        raise config_mod.ConfigError(f"无法读取 SQL 文件 {path}：{exc}") from exc

    statements = split_sql(content)
    if not statements:
        raise config_mod.ConfigError(f"SQL 文件没有可执行语句：{path}")
    return path, content, statements


def print_dry_run(console: Console, path: Path, content: str, statements: list[Statement]) -> None:
    """打印预演结果（不连库）。"""
    checksum = hashlib.sha256(content.encode("utf-8")).hexdigest()
    tables = extract_tables(content)
    console.info(
        console.paint(f"▶ 预演 {path}", "cyan")
        + f" （sha256 {checksum[:12]}，{len(statements)} 条语句）"
    )
    console.info(f"  受影响表：{format_table_list(tables)}")
    for statement in statements:
        console.info(f"  [{statement.index}] 行 {statement.start_line}  {statement.summary}")
    console.info("  （预演模式：未连接数据库，未执行任何语句）")


def print_outcome(console: Console, outcome) -> None:
    """打印单条语句的执行结果。"""
    suffix = f"影响 {outcome.rowcount} 行" if (outcome.rowcount or 0) >= 0 else "影响 — 行"
    console.info(
        f"  [{outcome.index}] 行 {outcome.start_line}  {outcome.summary}"
        f"    {outcome.elapsed_ms:.0f}ms  {suffix}"
    )
    if outcome.columns and outcome.rows:
        console.info("")
        console.plain(render_table(outcome.columns, outcome.rows))
        console.info("")
    elif outcome.columns:
        console.info(f"  （无数据行，{len(outcome.columns)} 列）")
    if outcome.truncated:
        console.warn(f"结果超过 {len(outcome.rows)} 行，已截断显示")


def report_failure(console: Console, path: Path, failure: ScriptFailure) -> None:
    """打印脚本失败详情。"""
    console.error(f"脚本执行失败：{path}")
    if failure.index is not None:
        console.error(f"  失败语句：第 {failure.index} 条，脚本第 {failure.start_line} 行")
        console.error(f"  语句摘要：{failure.summary}")
    if failure.sqlstate:
        console.error(f"  SQLSTATE：{failure.sqlstate}")
    console.error(f"  错误信息：{failure.message}")
    if failure.detail:
        console.error(f"  详细信息：{failure.detail}")
    if failure.hint:
        console.error(f"  提示：{failure.hint}")
    console.error("  事务已整体回滚，数据库未留下本次脚本的改动。")


def run_script(
    conn: psycopg.Connection,
    console: Console,
    path: Path,
    content: str,
    statements: list[Statement],
    args: argparse.Namespace,
    target: config_mod.DbTarget,
) -> int:
    """执行单个脚本，返回该脚本的退出码。"""
    checksum = hashlib.sha256(content.encode("utf-8")).hexdigest()
    tables = extract_tables(content)
    schema = args.migrations_schema

    # --- 幂等判定 ---
    by_checksum = migrations_mod.find_by_checksum(conn, schema, checksum)
    if by_checksum is not None and not args.force:
        console.info(
            console.paint(f"▶ 跳过 {path}", "yellow")
            + f" （已执行过，sha256 {by_checksum.checksum[:12]}，记录于 {by_checksum.applied_at}）"
        )
        return EXIT_OK

    by_name = migrations_mod.find_by_name(conn, schema, path.name)
    if by_name is not None and by_name.checksum != checksum and not args.force:
        console.error(
            f"脚本 {path.name} 已执行过，但内容与记录不一致"
            f"（记录指纹 {by_name.checksum[:12]}，当前 {checksum[:12]}）"
        )
        console.error("  如确认要重新执行，请加 --force。")
        return EXIT_SCRIPT_FAILED

    console.info(
        console.paint(f"▶ 执行 {path}", "cyan")
        + f" （sha256 {checksum[:12]}，{len(statements)} 条语句，目标 {target.redacted}）"
    )
    console.info(f"  受影响表：{format_table_list(tables)}")

    # --- 执行前备份 ---
    if not args.no_backup:
        try:
            result = backup_mod.backup_tables(
                conn,
                target,
                tables,
                str(path),
                checksum,
                Path(args.backup_dir),
            )
        except backup_mod.BackupError as exc:
            console.error(f"备份失败，已中止执行：{exc}")
            return EXIT_BACKUP
        if result.file is not None:
            console.info(f"  已备份表结构：{result.file}（{result.method}）")
        else:
            console.info("  无需备份（涉及的表尚不存在）")

    # --- 执行 ---
    started = time.perf_counter()
    # 前面的只读查询（备份、迁移记录查询）会隐式开启一个事务；先提交掉，
    # 否则接下来的 conn.transaction() 只会开 SAVEPOINT，外层事务在连接关闭时被回滚。
    conn.commit()
    try:
        with conn.transaction():
            outcomes = execute_statements(conn, statements, max_rows=args.max_rows)
            elapsed_ms = (time.perf_counter() - started) * 1000
            migrations_mod.record(
                conn,
                schema,
                path.name,
                checksum,
                int(elapsed_ms),
                len(statements),
            )
    except ScriptFailure as failure:
        report_failure(console, path, failure)
        return EXIT_SCRIPT_FAILED
    except psycopg.Error as exc:
        console.error(f"脚本 {path} 执行出错：{exc}")
        return EXIT_SCRIPT_FAILED

    for outcome in outcomes:
        print_outcome(console, outcome)
        if args.verbose:
            console.info("      SQL: " + statements[outcome.index - 1].sql.replace("\n", "\n           "))

    console.success(f"✔ {path} 提交成功（{len(outcomes)} 条语句，总耗时 {elapsed_ms:.0f}ms）")
    return EXIT_OK


def print_applied(console: Console, records: list[migrations_mod.MigrationRecord]) -> None:
    """打印已执行清单。"""
    if not records:
        console.info("（还没有已执行的迁移记录）")
        return
    rows = [
        (
            r.script_name,
            r.checksum[:12],
            r.applied_at,
            f"{r.execution_ms}ms" if r.execution_ms is not None else "-",
            r.statement_count if r.statement_count is not None else "-",
        )
        for r in records
    ]
    console.plain(render_table(["脚本", "指纹", "应用时间", "耗时", "语句数"], rows))


def main(argv: list[str] | None = None) -> int:
    """命令行入口，返回退出码。"""
    args = build_parser().parse_args(argv)
    console = Console(color=False if args.no_color else None, quiet=args.quiet)

    if not args.scripts and not args.list_applied:
        console.error("缺少 SQL 文件参数（用 --help 查看用法）")
        return EXIT_CONFIG

    try:
        # --- 读脚本、切分 ---
        loaded: list[tuple[Path, str, list[Statement]]] = []
        for script_text in args.scripts:
            loaded.append(load_script(script_text))

        if args.dry_run:
            for path, content, statements in loaded:
                print_dry_run(console, path, content, statements)
            return EXIT_OK

        # --- 连接配置 ---
        env_file: Path | None = None
        try:
            env_file = config_mod.find_env_file(args.env_file)
        except config_mod.ConfigError as exc:
            # 显式指定的环境文件不存在时，若进程环境里已有 DATABASE_URL 则继续
            if not os.environ.get("DATABASE_URL"):
                raise
            console.warn(f"{exc}；忽略该文件，改用进程环境变量 DATABASE_URL")
        url, source = config_mod.read_database_url(env_file, args.url)
        target = config_mod.parse_target(url)

        # --- 连接数据库 ---
        try:
            conn = connect(
                url,
                connect_timeout=args.connect_timeout,
                statement_timeout=args.statement_timeout,
            )
        except psycopg.OperationalError as exc:
            console.error(f"连接数据库失败（{source}）：{exc}")
            return EXIT_CONNECT
        except psycopg.Error as exc:
            console.error(f"连接数据库失败（{source}）：{exc}")
            return EXIT_CONNECT

        try:
            migrations_mod.ensure_table(conn, args.migrations_schema)

            if args.list_applied:
                print_applied(console, migrations_mod.list_applied(conn, args.migrations_schema))
                return EXIT_OK

            console.info(f"目标库：{target.redacted}（来源：{source}）")
            for path, content, statements in loaded:
                code = run_script(conn, console, path, content, statements, args, target)
                if code != EXIT_OK:
                    return code
            console.success("全部脚本执行完成")
            return EXIT_OK
        finally:
            conn.close()
    except config_mod.ConfigError as exc:
        console.error(str(exc))
        return EXIT_CONFIG
    except backup_mod.BackupError as exc:
        console.error(f"备份失败：{exc}")
        return EXIT_BACKUP


if __name__ == "__main__":
    sys.exit(main())
