"""表名提取单元测试。"""

from __future__ import annotations

from dbtool.tables import extract_tables, format_table_list


def test_extract_create_and_alter():
    sql = "CREATE TABLE orders (id int);\nALTER TABLE orders ADD COLUMN note text;"
    tables = extract_tables(sql)
    assert [(t.schema, t.name) for t in tables] == [(None, "orders")]


def test_extract_drop_with_if_exists():
    sql = "DROP TABLE IF EXISTS old_orders;"
    tables = extract_tables(sql)
    assert tables[0].name == "old_orders"


def test_extract_schema_qualified():
    sql = "CREATE TABLE rts.orders (id int);"
    tables = extract_tables(sql)
    assert tables[0].schema == "rts"
    assert tables[0].name == "orders"


def test_extract_quoted_identifier():
    sql = 'CREATE TABLE "my table" (id int);'
    tables = extract_tables(sql)
    assert tables[0].name == "my table"


def test_extract_ignores_comments():
    sql = "-- DROP TABLE ghost;\nCREATE TABLE real_table (id int);"
    tables = extract_tables(sql)
    assert [t.name for t in tables] == ["real_table"]


def test_extract_truncate_and_index():
    sql = "TRUNCATE orders;\nCREATE INDEX idx ON orders (id);"
    tables = extract_tables(sql)
    assert [t.name for t in tables] == ["orders"]


def test_extract_deduplicates():
    sql = "CREATE TABLE a (id int); ALTER TABLE a ADD COLUMN x int; DROP TABLE a;"
    tables = extract_tables(sql)
    assert len(tables) == 1


def test_format_table_list_uses_default_schema():
    sql = "CREATE TABLE orders (id int);"
    assert format_table_list(extract_tables(sql)) == "public.orders"
