"""执行前的表结构备份。

两条路径：

1. **pg_dump**（优先）：本机装了 `pg_dump` 时用它导出 `--schema-only` 结构，口令通过
   `PGPASSWORD` 环境变量传递，**不出现在命令行参数**里（`ps` 可见部分不含凭据）；
2. **pg_catalog 内省**（回退）：没有 `pg_dump` 时，从系统表读出列、约束、索引，
   重建一份「功能等价」的 `CREATE TABLE` / `ALTER TABLE` / `CREATE INDEX`。

内省重建的结果**不是原始 DDL 文本**，备份文件头部会写明该限制，仅作人工参考。
"""

from __future__ import annotations

import json
import shutil
import subprocess
from dataclasses import dataclass, field
from datetime import datetime
from pathlib import Path

import psycopg

from .config import DbTarget
from .tables import TableRef


class BackupError(Exception):
    """备份失败；调用方应中止执行（不改动数据库）。"""


@dataclass
class BackupResult:
    """一次备份的结果。"""

    file: Path | None = None
    method: str = ""
    tables: list[str] = field(default_factory=list)
    manifest: Path | None = None


def _pg_dump_env(target: DbTarget) -> dict[str, str]:
    """构造 pg_dump 的连接环境变量（口令走环境变量，不进 argv）。"""
    env = {
        "PGHOST": target.host or "",
        "PGPORT": str(target.port or 5432),
        "PGUSER": target.user or "",
        "PGPASSWORD": target.password or "",
        "PGDATABASE": target.database or "",
    }
    return {k: v for k, v in env.items() if v != ""}


def _dump_with_pg_dump(target: DbTarget, table: TableRef, tmp_file: Path) -> str | None:
    """用 pg_dump 导出单表结构，返回导出的文本；失败返回 None（由调用方回退内省重建）。"""
    cmd = [
        "pg_dump",
        "--schema-only",
        "--no-owner",
        "--no-privileges",
        f"--table={table.qualified}",
        f"--file={tmp_file}",
    ]
    env = _pg_dump_env(target)
    try:
        proc = subprocess.run(cmd, env=env, capture_output=True, text=True, timeout=120)
    except (OSError, subprocess.SubprocessError) as exc:
        raise BackupError(f"调用 pg_dump 失败：{exc}") from exc

    if proc.returncode != 0:
        # 不直接失败，交由调用方回退到内省重建
        tmp_file.unlink(missing_ok=True)
        return None
    text = tmp_file.read_text(encoding="utf-8")
    tmp_file.unlink(missing_ok=True)
    return text


def _server_version_num(conn: psycopg.Connection) -> int:
    with conn.cursor() as cur:
        cur.execute("SHOW server_version_num")
        row = cur.fetchone()
    return int(row[0]) if row else 0


def _introspect_table(conn: psycopg.Connection, table: TableRef, version_num: int) -> str:
    """从系统表重建一张表的结构语句。"""
    qualified = table.qualified
    with conn.cursor() as cur:
        if version_num >= 120000:
            cur.execute(
                """
                SELECT a.attname,
                       format_type(a.atttypid, a.atttypmod),
                       a.attnotnull,
                       pg_get_expr(d.adbin, d.adrelid),
                       a.attidentity,
                       a.attgenerated
                FROM pg_attribute a
                LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum
                WHERE a.attrelid = to_regclass(%s) AND a.attnum > 0 AND NOT a.attisdropped
                ORDER BY a.attnum
                """,
                (qualified,),
            )
        else:
            cur.execute(
                """
                SELECT a.attname,
                       format_type(a.atttypid, a.atttypmod),
                       a.attnotnull,
                       pg_get_expr(d.adbin, d.adrelid),
                       a.attidentity,
                   ''
                FROM pg_attribute a
                LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum
                WHERE a.attrelid = to_regclass(%s) AND a.attnum > 0 AND NOT a.attisdropped
                ORDER BY a.attnum
                """,
                (qualified,),
            )
        columns = cur.fetchall()

        cur.execute(
            """
            SELECT conname, pg_get_constraintdef(oid)
            FROM pg_constraint
            WHERE conrelid = to_regclass(%s) AND contype IN ('p', 'u', 'f', 'c')
            ORDER BY contype, conname
            """,
            (qualified,),
        )
        constraints = cur.fetchall()

        cur.execute(
            """
            SELECT indexname, indexdef
            FROM pg_indexes
            WHERE schemaname = %s AND tablename = %s
            ORDER BY indexname
            """,
            (table.schema, table.name),
        )
        indexes = cur.fetchall()

    if not columns:
        raise BackupError(f"表 {qualified} 不存在或没有可备份的列")

    lines = [f"CREATE TABLE {qualified} ("]
    column_defs = []
    for name, col_type, not_null, default, identity, generated in columns:
        parts = [f"    {name} {col_type}"]
        if identity in ("a", "d"):
            kind = "ALWAYS" if identity == "a" else "BY DEFAULT"
            parts.append(f"GENERATED {kind} AS IDENTITY")
        elif generated == "s":
            parts.append(f"GENERATED ALWAYS AS ({default}) STORED")
        elif default is not None:
            parts.append(f"DEFAULT {default}")
        if not_null:
            parts.append("NOT NULL")
        column_defs.append(" ".join(parts))
    lines.append(",\n".join(column_defs))
    lines.append(");")

    constraint_names = {row[0] for row in constraints}
    for name, definition in constraints:
        lines.append(f"ALTER TABLE {qualified} ADD CONSTRAINT {name} {definition};")

    for name, definition in indexes:
        if name in constraint_names:
            continue  # 主键/唯一约束已通过 ALTER TABLE 表达
        lines.append(f"{definition};")

    return "\n".join(lines) + "\n"


def _existing_tables(conn: psycopg.Connection, tables: list[TableRef]) -> list[TableRef]:
    """筛出数据库里已经存在的表（新建表无需备份）。"""
    existing: list[TableRef] = []
    with conn.cursor() as cur:
        for table in tables:
            cur.execute("SELECT to_regclass(%s) IS NOT NULL", (table.qualified,))
            row = cur.fetchone()
            if row and row[0]:
                existing.append(table)
    return existing


def backup_tables(
    conn: psycopg.Connection,
    target: DbTarget,
    tables: list[TableRef],
    script_name: str,
    script_checksum: str,
    backup_dir: Path,
) -> BackupResult:
    """备份脚本涉及的表结构，返回备份结果。"""
    existing = _existing_tables(conn, tables)
    # 只读查询会隐式开启事务，提交掉，避免与后续的业务事务嵌套
    conn.commit()
    if not existing:
        return BackupResult(file=None, method="无需备份", tables=[])

    backup_dir.mkdir(parents=True, exist_ok=True)
    stamp = datetime.now().strftime("%Y%m%d_%H%M%S")
    stem = Path(script_name).stem or "script"
    out_file = backup_dir / f"{stem}_{stamp}.sql"
    tmp_file = backup_dir / f".{stem}_{stamp}.pg_dump.tmp"

    methods: set[str] = set()
    chunks: list[str] = []
    pg_dump_ok = shutil.which("pg_dump") is not None
    version_num = _server_version_num(conn)

    for table in existing:
        text: str | None = None
        if pg_dump_ok:
            try:
                text = _dump_with_pg_dump(target, table, tmp_file)
            except BackupError:
                text = None
        if text is not None:
            methods.add("pg_dump")
            chunks.append(text)
        else:
            try:
                methods.add("pg_catalog 内省重建")
                chunks.append(_introspect_table(conn, table, version_num))
            except psycopg.Error as exc:
                raise BackupError(f"读取表 {table.qualified} 的结构失败：{exc}") from exc

    method = "pg_dump" if methods == {"pg_dump"} else "pg_catalog 内省重建"

    header = [
        "-- db_caller 表结构备份",
        f"-- 备份时间: {datetime.now().isoformat(timespec='seconds')}",
        f"-- 来源脚本: {script_name}",
        f"-- 来源指纹: sha256:{script_checksum}",
        f"-- 受影响表: {', '.join(t.qualified for t in existing)}",
        f"-- 备份方式: {method}",
    ]
    if method != "pg_dump":
        header.append("-- 注意: 本文件由 pg_catalog 内省重建，不是原始 DDL 文本，仅供人工参考。")
    body = "\n".join(header) + "\n\n"

    try:
        out_file.write_text(body + "\n".join(chunks), encoding="utf-8")
    except OSError as exc:
        raise BackupError(f"写入备份文件失败：{exc}") from exc

    manifest = {
        "generated_at": datetime.now().isoformat(timespec="seconds"),
        "source_script": script_name,
        "source_sha256": script_checksum,
        "method": method,
        "tables": [t.qualified for t in existing],
        "backup_file": out_file.name,
    }
    manifest_file = backup_dir / f"{stem}_{stamp}.manifest.json"
    manifest_file.write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )

    return BackupResult(
        file=out_file, method=method, tables=[t.qualified for t in existing], manifest=manifest_file
    )
