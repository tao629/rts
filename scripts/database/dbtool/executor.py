"""执行引擎：连接数据库，在单个事务里逐条执行语句并回显结果。"""

from __future__ import annotations

import time
from dataclasses import dataclass, field

import psycopg

from .splitter import Statement


@dataclass
class StatementOutcome:
    """一条语句的执行结果。"""

    index: int
    start_line: int
    summary: str
    elapsed_ms: float
    rowcount: int | None = None
    columns: list[str] = field(default_factory=list)
    rows: list[tuple] = field(default_factory=list)
    truncated: bool = False


class ScriptFailure(Exception):
    """脚本执行失败（事务会被回滚）。"""

    def __init__(
        self,
        message: str,
        *,
        index: int | None = None,
        start_line: int | None = None,
        summary: str | None = None,
        sqlstate: str | None = None,
        detail: str | None = None,
        hint: str | None = None,
    ) -> None:
        super().__init__(message)
        self.message = message
        self.index = index
        self.start_line = start_line
        self.summary = summary
        self.sqlstate = sqlstate
        self.detail = detail
        self.hint = hint


def connect(url: str, connect_timeout: int = 10, statement_timeout: int = 0) -> psycopg.Connection:
    """建立连接并设置会话级超时。"""
    conn = psycopg.connect(url, connect_timeout=connect_timeout)
    with conn.cursor() as cur:
        # SET 不支持参数占位符，用 set_config 传入带单位的字符串
        cur.execute(
            "SELECT set_config('statement_timeout', %s, false)",
            (f"{statement_timeout * 1000}ms",),
        )
    conn.commit()
    return conn


def execute_statements(
    conn: psycopg.Connection, statements: list[Statement], max_rows: int = 100
) -> list[StatementOutcome]:
    """逐条执行语句（由调用方负责事务边界）。

    任一语句失败时抛出 `ScriptFailure`，事务由外层回滚。
    """
    outcomes: list[StatementOutcome] = []

    for statement in statements:
        started = time.perf_counter()
        try:
            with conn.cursor() as cur:
                cur.execute(statement.sql)
                elapsed_ms = (time.perf_counter() - started) * 1000

                outcome = StatementOutcome(
                    index=statement.index,
                    start_line=statement.start_line,
                    summary=statement.summary,
                    elapsed_ms=elapsed_ms,
                )

                if cur.description is not None:
                    outcome.columns = [col.name for col in cur.description]
                    fetched = cur.fetchmany(max_rows + 1)
                    if len(fetched) > max_rows:
                        outcome.rows = fetched[:max_rows]
                        outcome.truncated = True
                    else:
                        outcome.rows = fetched
                    outcome.rowcount = len(outcome.rows)
                else:
                    outcome.rowcount = cur.rowcount if cur.rowcount is not None else 0

                outcomes.append(outcome)
        except psycopg.Error as exc:
            elapsed_ms = (time.perf_counter() - started) * 1000
            diag = exc.diag
            raise ScriptFailure(
                str(exc).strip(),
                index=statement.index,
                start_line=statement.start_line,
                summary=statement.summary,
                sqlstate=exc.sqlstate,
                detail=getattr(diag, "message_detail", None),
                hint=getattr(diag, "message_hint", None),
            ) from exc

    return outcomes
