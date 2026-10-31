"""输出：颜色、日志格式与表格渲染。

颜色只在 TTY 下启用（便于 CI 日志阅读）；`--no-color` 或 `NO_COLOR` 环境变量可强制关闭。
"""

from __future__ import annotations

import os
import sys
from typing import Iterable, Sequence

_RESET = "\033[0m"
_COLORS = {
    "red": "\033[31m",
    "green": "\033[32m",
    "yellow": "\033[33m",
    "blue": "\033[34m",
    "cyan": "\033[36m",
    "grey": "\033[90m",
}


class Console:
    """带颜色开关的输出器。"""

    def __init__(self, color: bool | None = None, quiet: bool = False) -> None:
        if color is None:
            color = sys.stdout.isatty() and not os.environ.get("NO_COLOR")
        self.color = bool(color)
        self.quiet = quiet

    def paint(self, text: str, color: str) -> str:
        if not self.color or color not in _COLORS:
            return text
        return f"{_COLORS[color]}{text}{_RESET}"

    def info(self, text: str = "") -> None:
        if not self.quiet:
            print(text)

    def warn(self, text: str) -> None:
        sys.stdout.flush()  # 保证与 stdout 的日志顺序一致
        print(self.paint("警告: ", "yellow") + text, file=sys.stderr)

    def error(self, text: str) -> None:
        sys.stdout.flush()
        print(self.paint("错误: ", "red") + text, file=sys.stderr)

    def success(self, text: str) -> None:
        if not self.quiet:
            print(self.paint(text, "green"))

    def step(self, text: str) -> None:
        if not self.quiet:
            print(self.paint(text, "cyan"))

    def plain(self, text: str = "") -> None:
        """不受 --quiet 影响的输出（结果表格等）。"""
        print(text)


def _cell(value: object) -> str:
    if value is None:
        return "NULL"
    return str(value)


def render_table(headers: Sequence[str], rows: Iterable[Sequence[object]]) -> str:
    """把结果集渲染成对齐的文本表格。"""
    row_list = [[_cell(v) for v in row] for row in rows]
    widths = [len(h) for h in headers]
    for row in row_list:
        for idx, cell in enumerate(row):
            if idx < len(widths):
                widths[idx] = max(widths[idx], len(cell))

    def render_row(cells: Sequence[str]) -> str:
        parts = []
        for idx, cell in enumerate(cells):
            width = widths[idx] if idx < len(widths) else len(cell)
            parts.append(cell.ljust(width))
        return "  ".join(parts).rstrip()

    lines = [render_row(list(headers)), "  ".join("-" * w for w in widths)]
    lines.extend(render_row(row) for row in row_list)
    return "\n".join(lines)
