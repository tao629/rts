"""SQL 脚本切分：把一个脚本文件切成一条条可单独执行的语句，并记录起始行号。

为什么自己写而不直接用 sqlparse：

- 需要精确的「脚本内起始行号」，用于失败定位；
- 需要正确处理 PostgreSQL 的 dollar-quoted 字符串（`$$ … $$` / `$tag$ … $tag$`），
  里面的分号不是语句分隔符；
- 需要支持**嵌套**的块注释（PostgreSQL 允许 `/* /* */ */`）。

状态机在字符级别扫描，状态包括：普通文本、行注释、块注释（带嵌套深度）、
单引号字符串（支持 `''` 与 `E''` 反斜杠转义）、双引号标识符、dollar-quoted 字符串。
只有处于「普通文本」状态时遇到的 `;` 才是语句分隔符。
"""

from __future__ import annotations

import re
from dataclasses import dataclass

# dollar-quoted 标签：$tag$ 或 $$，tag 为标识符（可为空）
_DOLLAR_TAG_RE = re.compile(r"\$([A-Za-z_][A-Za-z0-9_]*)?\$")
# 标识符：裸标识符或双引号标识符
_IDENT = r'(?:"[^"]*"|[A-Za-z_][A-Za-z0-9_$]*)'


class SqlSplitError(Exception):
    """脚本内容无法切分（如 dollar-quoted 未闭合）。"""


@dataclass(frozen=True)
class Statement:
    """一条待执行的语句。"""

    index: int  # 1 起的序号
    sql: str  # 语句文本（不含结尾分号）
    start_line: int  # 脚本内起始行号（1 起）
    summary: str  # 首行摘要，用于日志


def _summarize(sql: str, limit: int = 120) -> str:
    """取语句首行（去注释）作为摘要。"""
    for raw_line in sql.splitlines():
        line = raw_line.strip()
        if not line:
            continue
        if line.startswith("--"):
            continue
        if line.startswith("/*"):
            continue
        if len(line) > limit:
            return line[: limit - 1] + "…"
        return line
    stripped = sql.strip().splitlines()
    return stripped[0][:limit] if stripped else ""


def split_sql(text: str) -> list[Statement]:
    """把脚本文本切成语句列表。

    只包含注释与空白的片段会被忽略；文件末尾没有分号时，最后一段也算一条语句。
    """
    statements: list[Statement] = []
    buf: list[str] = []
    code_line: int | None = None  # 当前语句第一条有效 SQL 的行号
    line = 1
    i = 0
    length = len(text)

    state = "normal"  # normal/line_comment/block_comment/single/double/dollar
    block_depth = 0
    dollar_tag = ""
    escape_string = False  # 是否处于 E'' 转义字符串中

    def flush() -> None:
        """结束当前语句：非空且含有效 SQL 时追加到结果。"""
        nonlocal buf, code_line
        content = "".join(buf)
        if content.strip() and code_line is not None:
            statements.append(
                Statement(
                    index=len(statements) + 1,
                    sql=content.strip(),
                    start_line=code_line,
                    summary=_summarize(content),
                )
            )
        buf = []
        code_line = None

    while i < length:
        ch = text[i]
        nxt = text[i + 1] if i + 1 < length else ""

        if state == "line_comment":
            buf.append(ch)
            if ch == "\n":
                line += 1
                state = "normal"
            i += 1
            continue

        if state == "block_comment":
            buf.append(ch)
            if ch == "\n":
                line += 1
                i += 1
                continue
            if ch == "/" and nxt == "*":
                buf.append(nxt)
                block_depth += 1
                i += 2
                continue
            if ch == "*" and nxt == "/":
                buf.append(nxt)
                block_depth -= 1
                i += 2
                if block_depth == 0:
                    state = "normal"
                continue
            i += 1
            continue

        if state == "single":
            buf.append(ch)
            if ch == "\\" and escape_string and nxt:
                # E'...' 里的反斜杠转义下一个字符
                buf.append(nxt)
                if nxt == "\n":
                    line += 1
                i += 2
                continue
            if ch == "'":
                if nxt == "'":  # '' 转义
                    buf.append(nxt)
                    i += 2
                    continue
                state = "normal"
                escape_string = False
            if ch == "\n":
                line += 1
            i += 1
            continue

        if state == "double":
            buf.append(ch)
            if ch == '"':
                if nxt == '"':  # "" 转义
                    buf.append(nxt)
                    i += 2
                    continue
                state = "normal"
            if ch == "\n":
                line += 1
            i += 1
            continue

        if state == "dollar":
            if text.startswith(dollar_tag, i):
                buf.append(dollar_tag)
                i += len(dollar_tag)
                state = "normal"
                continue
            buf.append(ch)
            if ch == "\n":
                line += 1
            i += 1
            continue

        # --- 普通文本状态 ---
        if ch == "-" and nxt == "-":
            buf.append(ch)
            buf.append(nxt)
            i += 2
            state = "line_comment"
            continue

        if ch == "/" and nxt == "*":
            buf.append(ch)
            buf.append(nxt)
            i += 2
            state = "block_comment"
            block_depth = 1
            continue

        if ch == "'":
            # 判断是否是 E'' 转义字符串
            escape_string = bool(buf) and buf[-1] in ("E", "e") and not (
                len(buf) >= 2 and (buf[-2].isalnum() or buf[-2] == "_" or buf[-2] == "$")
            )
            buf.append(ch)
            i += 1
            state = "single"
            continue

        if ch == '"':
            buf.append(ch)
            i += 1
            state = "double"
            continue

        if ch == "$":
            match = _DOLLAR_TAG_RE.match(text, i)
            if match:
                dollar_tag = match.group(0)
                buf.append(dollar_tag)
                i += len(dollar_tag)
                state = "dollar"
                continue

        if ch == ";":
            flush()
            i += 1
            continue

        if not ch.isspace() and code_line is None:
            code_line = line
        buf.append(ch)
        if ch == "\n":
            line += 1
        i += 1

    if state == "dollar":
        raise SqlSplitError(f"脚本中的 dollar-quoted 字符串 {dollar_tag} 没有闭合")
    if state == "block_comment":
        raise SqlSplitError("脚本中的块注释 /* 没有闭合")

    flush()
    return statements


def strip_comments(text: str) -> str:
    """去掉注释，保留字符串字面量内容（用于表名提取，避免注释里的假语句）。"""
    out: list[str] = []
    i = 0
    length = len(text)
    state = "normal"
    block_depth = 0
    dollar_tag = ""

    while i < length:
        ch = text[i]
        nxt = text[i + 1] if i + 1 < length else ""

        if state == "line_comment":
            if ch == "\n":
                state = "normal"
                out.append(ch)
            i += 1
            continue

        if state == "block_comment":
            if ch == "/" and nxt == "*":
                block_depth += 1
                i += 2
                continue
            if ch == "*" and nxt == "/":
                block_depth -= 1
                i += 2
                if block_depth == 0:
                    state = "normal"
                continue
            i += 1
            continue

        if state in ("single", "double"):
            out.append(ch)
            quote = ch
            if ch == quote and nxt == quote:
                out.append(nxt)
                i += 2
                continue
            if ch == quote:
                state = "normal"
            i += 1
            continue

        if state == "dollar":
            if text.startswith(dollar_tag, i):
                out.append(dollar_tag)
                i += len(dollar_tag)
                state = "normal"
                continue
            out.append(ch)
            i += 1
            continue

        if ch == "-" and nxt == "-":
            state = "line_comment"
            i += 2
            continue
        if ch == "/" and nxt == "*":
            state = "block_comment"
            block_depth = 1
            i += 2
            continue
        if ch == "'":
            state = "single"
            out.append(ch)
            i += 1
            continue
        if ch == '"':
            state = "double"
            out.append(ch)
            i += 1
            continue
        if ch == "$":
            match = _DOLLAR_TAG_RE.match(text, i)
            if match:
                dollar_tag = match.group(0)
                state = "dollar"
                out.append(dollar_tag)
                i += len(dollar_tag)
                continue
        out.append(ch)
        i += 1

    return "".join(out)
