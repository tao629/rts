"""配置模块单元测试：环境文件查找、连接串解析与脱敏。"""

from __future__ import annotations

from pathlib import Path

import pytest

from dbtool.config import ConfigError, find_env_file, parse_target, read_database_url, redact_url


def test_redact_removes_password():
    url = "postgresql://app:s3cr3t@db.example.com:5432/rts"
    redacted = redact_url(url)
    assert "s3cr3t" not in redacted
    assert redacted == "postgresql://app@db.example.com:5432/rts"


def test_redact_handles_unencoded_at_in_password():
    url = "postgresql://app:p@ss@db.example.com:5432/rts"
    redacted = redact_url(url)
    assert "p@ss" not in redacted
    assert redacted.endswith("/rts")


def test_parse_target_extracts_parts():
    target = parse_target("postgresql://app:s3cr3t@db.example.com:5432/rts")
    assert target.scheme == "postgresql"
    assert target.user == "app"
    assert target.password == "s3cr3t"
    assert target.host == "db.example.com"
    assert target.port == 5432
    assert target.database == "rts"


def test_parse_target_rejects_non_postgres():
    with pytest.raises(ConfigError):
        parse_target("mysql://root@127.0.0.1:3306/rts")


def test_parse_target_rejects_missing_scheme():
    with pytest.raises(ConfigError):
        parse_target("app@127.0.0.1/rts")


def test_target_redacted_hides_password():
    target = parse_target("postgresql://app:s3cr3t@db:5432/rts")
    assert "s3cr3t" not in target.redacted


def test_find_env_file_walks_up(tmp_path: Path):
    root = tmp_path / "repo"
    nested = root / "scripts" / "database"
    nested.mkdir(parents=True)
    env = root / ".env"
    env.write_text("DATABASE_URL=postgresql://u:p@h:5432/d\n", encoding="utf-8")
    assert find_env_file(None, start=nested) == env


def test_find_env_file_explicit_missing(tmp_path: Path):
    with pytest.raises(ConfigError):
        find_env_file(str(tmp_path / "nope.env"))


def test_read_database_url_prefers_process_env(monkeypatch, tmp_path: Path):
    env = tmp_path / ".env"
    env.write_text("DATABASE_URL=postgresql://file@host/db\n", encoding="utf-8")
    monkeypatch.setenv("DATABASE_URL", "postgresql://process@host/db")
    url, source = read_database_url(env)
    assert url == "postgresql://process@host/db"
    assert "进程环境变量" in source


def test_read_database_url_falls_back_to_file(monkeypatch, tmp_path: Path):
    monkeypatch.delenv("DATABASE_URL", raising=False)
    env = tmp_path / ".env"
    env.write_text("DATABASE_URL=postgresql://file@host/db\n", encoding="utf-8")
    url, source = read_database_url(env)
    assert url == "postgresql://file@host/db"
    assert "环境文件" in source


def test_read_database_url_missing(monkeypatch, tmp_path: Path):
    monkeypatch.delenv("DATABASE_URL", raising=False)
    with pytest.raises(ConfigError):
        read_database_url(tmp_path / ".env")


def test_url_override_wins(monkeypatch, tmp_path: Path):
    monkeypatch.setenv("DATABASE_URL", "postgresql://process@host/db")
    url, source = read_database_url(tmp_path / ".env", url_override="postgresql://override@host/db")
    assert url == "postgresql://override@host/db"
    assert source == "--url 参数"
