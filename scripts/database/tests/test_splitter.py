"""切分器单元测试：语句边界、注释、字符串、dollar-quoted、行号。"""

from __future__ import annotations

import pytest

from dbtool.splitter import SqlSplitError, split_sql, strip_comments


def test_two_simple_statements():
    statements = split_sql("CREATE TABLE a (id int); DROP TABLE a;")
    assert len(statements) == 2
    assert statements[0].sql == "CREATE TABLE a (id int)"
    assert statements[1].sql == "DROP TABLE a"
    assert statements[0].index == 1 and statements[1].index == 2


def test_semicolon_inside_line_comment_is_not_a_boundary():
    statements = split_sql("-- 这里有个分号 ; 不要切\nSELECT 1;")
    assert len(statements) == 1
    assert statements[0].sql.endswith("SELECT 1")


def test_semicolon_inside_block_comment_is_not_a_boundary():
    statements = split_sql("/* 注释 ; 里的分号 */ SELECT 1;")
    assert len(statements) == 1


def test_nested_block_comment():
    statements = split_sql("/* 外层 /* 内层 ; */ 结束 */ SELECT 1;")
    assert len(statements) == 1
    assert statements[0].sql.strip().endswith("SELECT 1")


def test_semicolon_inside_string_literal():
    statements = split_sql("INSERT INTO t VALUES ('a;b');")
    assert len(statements) == 1


def test_escaped_quote_in_string():
    statements = split_sql("INSERT INTO t VALUES ('it''s; fine');")
    assert len(statements) == 1


def test_dollar_quoted_body_is_not_split():
    sql = """
CREATE FUNCTION add(a int, b int) RETURNS int AS $$
BEGIN
    RETURN a + b;
END;
$$ LANGUAGE plpgsql;
SELECT 1;
"""
    statements = split_sql(sql)
    assert len(statements) == 2
    assert "RETURN a + b;" in statements[0].sql
    assert statements[1].sql == "SELECT 1"


def test_tagged_dollar_quoted_body_is_not_split():
    sql = "CREATE FUNCTION f() RETURNS void AS $body$ BEGIN; END; $body$ LANGUAGE plpgsql;"
    statements = split_sql(sql)
    assert len(statements) == 1


def test_unclosed_dollar_quote_raises():
    with pytest.raises(SqlSplitError):
        split_sql("CREATE FUNCTION f() AS $$ SELECT 1;")


def test_start_line_is_reported():
    sql = "-- 注释\n\nCREATE TABLE a (id int);\n\nSELECT 1;\n"
    statements = split_sql(sql)
    assert statements[0].start_line == 3
    assert statements[1].start_line == 5


def test_trailing_statement_without_semicolon():
    statements = split_sql("SELECT 1;\nSELECT 2")
    assert len(statements) == 2
    assert statements[1].sql == "SELECT 2"


def test_comment_only_script_yields_nothing():
    assert split_sql("-- 只有注释\n/* 和块注释 */\n") == []


def test_statement_keeping_leading_comment():
    statements = split_sql("-- 建表\nCREATE TABLE a (id int);")
    assert len(statements) == 1
    assert statements[0].summary == "CREATE TABLE a (id int)"


def test_strip_comments_keeps_string_content():
    text = "SELECT '-- 不是注释' -- 真注释\n"
    assert "-- 不是注释" in strip_comments(text)
    assert "真注释" not in strip_comments(text)
