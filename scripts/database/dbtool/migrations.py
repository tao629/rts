"""迁移记录：幂等判定与已执行清单。

记录表 `schema_migrations`（默认在 `public` schema，缺表时自动创建）记录每个已执行脚本的
「文件名 + 内容 SHA-256 指纹」，用于避免同一个迁移脚本被重复执行。

记录写入与脚本的 DDL 在**同一个事务**里提交，保证「库改了但没记账」不会发生。
"""

from __future__ import annotations

from dataclasses import dataclass

import psycopg
from psycopg import sql

TABLE_NAME = "schema_migrations"


class MigrationConflict(Exception):
    """同名脚本内容已被改动（指纹不一致），需要 `--force` 才能强制执行。"""


@dataclass(frozen=True)
class MigrationRecord:
    """一条迁移记录。"""

    id: int
    script_name: str
    checksum: str
    applied_at: str
    execution_ms: int | None
    statement_count: int | None


def ensure_table(conn: psycopg.Connection, schema: str) -> None:
    """确保迁移记录表存在（单独提交，不与业务事务混在一起）。"""
    with conn.cursor() as cur:
        cur.execute(sql.SQL("CREATE SCHEMA IF NOT EXISTS {}").format(sql.Identifier(schema)))
        cur.execute(
            sql.SQL(
                """
                CREATE TABLE IF NOT EXISTS {}.{} (
                    id              serial PRIMARY KEY,
                    script_name     text        NOT NULL,
                    checksum        char(64)    NOT NULL,
                    applied_at      timestamptz NOT NULL DEFAULT now(),
                    execution_ms    integer,
                    statement_count integer
                )
                """
            ).format(sql.Identifier(schema), sql.Identifier(TABLE_NAME))
        )
        cur.execute(
            sql.SQL(
                "CREATE UNIQUE INDEX IF NOT EXISTS {} ON {}.{} (checksum)"
            ).format(
                sql.Identifier(f"{TABLE_NAME}_checksum_idx"),
                sql.Identifier(schema),
                sql.Identifier(TABLE_NAME),
            )
        )
    conn.commit()


def find_by_checksum(conn: psycopg.Connection, schema: str, checksum: str) -> MigrationRecord | None:
    """按指纹查找已执行记录。"""
    with conn.cursor() as cur:
        cur.execute(
            sql.SQL(
                """
                SELECT id, script_name, checksum, applied_at, execution_ms, statement_count
                FROM {}.{}
                WHERE checksum = %s
                """
            ).format(sql.Identifier(schema), sql.Identifier(TABLE_NAME)),
            (checksum,),
        )
        row = cur.fetchone()
    if row is None:
        return None
    return MigrationRecord(
        id=row[0],
        script_name=row[1],
        checksum=row[2].strip(),
        applied_at=row[3].isoformat(),
        execution_ms=row[4],
        statement_count=row[5],
    )


def find_by_name(conn: psycopg.Connection, schema: str, script_name: str) -> MigrationRecord | None:
    """按脚本名查找最近的已执行记录（用于检测「同名但内容被改动」）。"""
    with conn.cursor() as cur:
        cur.execute(
            sql.SQL(
                """
                SELECT id, script_name, checksum, applied_at, execution_ms, statement_count
                FROM {}.{}
                WHERE script_name = %s
                ORDER BY applied_at DESC, id DESC
                LIMIT 1
                """
            ).format(sql.Identifier(schema), sql.Identifier(TABLE_NAME)),
            (script_name,),
        )
        row = cur.fetchone()
    if row is None:
        return None
    return MigrationRecord(
        id=row[0],
        script_name=row[1],
        checksum=row[2].strip(),
        applied_at=row[3].isoformat(),
        execution_ms=row[4],
        statement_count=row[5],
    )


def record(
    conn: psycopg.Connection,
    schema: str,
    script_name: str,
    checksum: str,
    execution_ms: int,
    statement_count: int,
) -> None:
    """写入一条迁移记录（调用方负责提交事务）。

    用 `ON CONFLICT (checksum) DO UPDATE`：同一份脚本用 `--force` 重跑时，
    更新原记录（应用时间、耗时），而不是因为指纹唯一索引冲突而失败。
    """
    with conn.cursor() as cur:
        cur.execute(
            sql.SQL(
                """
                INSERT INTO {}.{} (script_name, checksum, execution_ms, statement_count)
                VALUES (%s, %s, %s, %s)
                ON CONFLICT (checksum) DO UPDATE
                SET script_name     = EXCLUDED.script_name,
                    applied_at      = now(),
                    execution_ms    = EXCLUDED.execution_ms,
                    statement_count = EXCLUDED.statement_count
                """
            ).format(sql.Identifier(schema), sql.Identifier(TABLE_NAME)),
            (script_name, checksum, execution_ms, statement_count),
        )


def list_applied(conn: psycopg.Connection, schema: str) -> list[MigrationRecord]:
    """列出全部已执行记录（按应用时间升序）。"""
    with conn.cursor() as cur:
        cur.execute(
            sql.SQL(
                """
                SELECT id, script_name, checksum, applied_at, execution_ms, statement_count
                FROM {}.{}
                ORDER BY applied_at, id
                """
            ).format(sql.Identifier(schema), sql.Identifier(TABLE_NAME))
        )
        rows = cur.fetchall()
    return [
        MigrationRecord(
            id=row[0],
            script_name=row[1],
            checksum=row[2].strip(),
            applied_at=row[3].isoformat(),
            execution_ms=row[4],
            statement_count=row[5],
        )
        for row in rows
    ]
