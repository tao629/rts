"""输出渲染单元测试。"""

from __future__ import annotations

from dbtool.output import Console, render_table


def test_render_table_aligns_columns():
    text = render_table(["id", "name"], [(1, "甲"), (22, "乙")])
    lines = text.splitlines()
    assert lines[0].startswith("id  name")
    assert set(lines[1]) <= {"-", " "}
    assert lines[2].startswith("1   甲")


def test_render_table_shows_null():
    text = render_table(["note"], [(None,)])
    assert "NULL" in text


def test_render_table_without_rows():
    text = render_table(["id", "name"], [])
    assert len(text.splitlines()) == 2


def test_console_paint_without_color():
    console = Console(color=False)
    assert console.paint("文本", "red") == "文本"


def test_console_paint_with_color():
    console = Console(color=True)
    assert "\033[" in console.paint("文本", "red")
