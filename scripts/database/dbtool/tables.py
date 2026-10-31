"""从 SQL 脚本中提取「受影响的表」，用于执行前的清单提示与结构备份。

只关心表级 DDL：`CREATE TABLE` / `ALTER TABLE` / `DROP TABLE` / `TRUNCATE`。
视图、索引等不进入备份范围（`CREATE INDEX ... ON 表` 会顺带带出宿主表，方便备份）。
"""

from __future__ import annotations

import re
from dataclasses import dataclass

from .splitter import strip_comments

_IDENT = r'(?:"[^"]*"|[A-Za-z_][A-Za-z0-9_$]*)'

# CREATE TABLE / ALTER TABLE / DROP TABLE / TRUNCATE 后的表名
_TABLE_RE = re.compile(
    r"\b(?:create\s+table|alter\s+table|drop\s+table|truncate)\s+"
    r"(?:if\s+not\s+exists\s+|if\s+exists\s+|only\s+|table\s+|temp\s+|temporary\s+|unlogged\s+)*"
    rf"(?P<name>{_IDENT}(?:\s*\.\s*{_IDENT})*)",
    re.IGNORECASE,
)

# CREATE INDEX ... ON 表
_INDEX_ON_RE = re.compile(
    rf"\bcreate\s+(?:unique\s+)?index\s+(?:concurrently\s+)?(?:if\s+not\s+exists\s+)?"
    rf"{_IDENT}\s+on\s+(?P<name>{_IDENT}(?:\s*\.\s*{_IDENT})*)",
    re.IGNORECASE,
)


@dataclass(frozen=True)
class TableRef:
    """表引用（schema 缺省时为 None，执行时按 search_path 解析）。"""

    schema: str | None
    name: str

    @property
    def qualified(self) -> str:
        return f"{self.schema}.{self.name}" if self.schema else self.name

    def resolve(self, default_schema: str = "public") -> "TableRef":
        """补齐缺省 schema。"""
        return self if self.schema else TableRef(default_schema, self.name)


def _parse_qualified(raw: str) -> TableRef:
    parts = [p.strip() for p in raw.split(".")]
    parts = [p[1:-1] if len(p) >= 2 and p.startswith('"') and p.endswith('"') else p for p in parts]
    if len(parts) == 1:
        return TableRef(None, parts[0])
    return TableRef(parts[-2], parts[-1])


def extract_tables(sql_text: str) -> list[TableRef]:
    """提取脚本涉及的表（去重，保持出现顺序）。"""
    cleaned = strip_comments(sql_text)
    found: list[TableRef] = []
    seen: set[tuple[str | None, str]] = set()

    for match in _TABLE_RE.finditer(cleaned):
        ref = _parse_qualified(match.group("name"))
        key = (ref.schema, ref.name)
        if key not in seen:
            seen.add(key)
            found.append(ref)

    for match in _INDEX_ON_RE.finditer(cleaned):
        ref = _parse_qualified(match.group("name"))
        key = (ref.schema, ref.name)
        if key not in seen:
            seen.add(key)
            found.append(ref)

    return found


def format_table_list(tables: list[TableRef], default_schema: str = "public") -> str:
    """把表列表格式化成一行日志文本。"""
    if not tables:
        return "（无）"
    return "、".join(t.resolve(default_schema).qualified for t in tables)
