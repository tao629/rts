"""连接配置：`.env` 查找与载入、`DATABASE_URL` 读取、连接串脱敏。

设计取舍：

- 不 `source` 环境文件，只按文本解析（`python-dotenv`），避免环境文件里的内容被当成代码执行；
- 环境文件里的变量**不写回**进程环境，因此「值来自进程环境」与「值来自文件」能区分清楚；
- 任何对外输出（日志、异常）里的连接串都经 `redact_url()` 脱敏，口令永不出现。
"""

from __future__ import annotations

import os
from dataclasses import dataclass
from pathlib import Path
from urllib.parse import urlsplit

from dotenv import dotenv_values

# 本工具只支持 PostgreSQL
ALLOWED_SCHEMES = ("postgresql", "postgres")


class ConfigError(Exception):
    """配置类错误（缺环境文件、缺 DATABASE_URL、URL 不受支持等），对应退出码 2。"""


@dataclass(frozen=True)
class DbTarget:
    """连接串解析结果（口令单独存放，便于脱敏输出）。"""

    scheme: str
    user: str | None
    password: str | None
    host: str | None
    port: int | None
    database: str | None

    @property
    def redacted(self) -> str:
        """脱敏后的目标库描述，形如 `postgresql://app@host:5432/rts`。"""
        desc = f"{self.scheme}://"
        if self.user:
            desc += f"{self.user}@"
        if self.host:
            desc += self.host
        if self.port:
            desc += f":{self.port}"
        if self.database:
            desc += f"/{self.database}"
        return desc


def find_env_file(explicit: str | None = None, start: Path | None = None) -> Path | None:
    """查找环境文件。

    优先使用 `explicit`；否则从 `start`（默认脚本所在目录）逐级向上找 `.env`，
    再从当前工作目录逐级向上找。找不到返回 `None`。
    """
    if explicit:
        path = Path(explicit).expanduser()
        if not path.is_file():
            raise ConfigError(f"找不到环境文件：{path}")
        return path

    bases = [start] if start is not None else [Path(__file__).resolve().parent]
    bases.append(Path.cwd())
    for base in bases:
        for directory in (base, *base.parents):
            candidate = directory / ".env"
            if candidate.is_file():
                return candidate
    return None


def load_env_file(path: Path | None) -> dict[str, str]:
    """读取环境文件里的变量（只解析，不执行、不写回进程环境）。

    刻意**不**把文件内容写进 `os.environ`：写进去之后就无法区分「值来自进程环境」还是
    「值来自文件」，来源提示会失真；本工具只需要 `DATABASE_URL` 一个变量。
    """
    if path is None:
        return {}
    return {k: v for k, v in dotenv_values(path).items() if v is not None}


def read_database_url(env_file: Path | None = None, url_override: str | None = None) -> tuple[str, str]:
    """返回 `(连接串, 来源说明)`。

    优先级：`--url` > 进程环境变量 `DATABASE_URL` > 环境文件中的 `DATABASE_URL`。
    """
    if url_override:
        return url_override, "--url 参数"

    from_process = os.environ.get("DATABASE_URL")
    if from_process:
        return from_process, "进程环境变量 DATABASE_URL"

    if env_file is not None:
        values = dotenv_values(env_file)
        value = values.get("DATABASE_URL")
        if value:
            return value.strip(), f"环境文件 {env_file}"

    raise ConfigError(
        "未找到 DATABASE_URL：请在 .env 中配置，或用 --env-file 指定环境文件，或用 --url 直接给出连接串"
    )


def parse_target(url: str) -> DbTarget:
    """解析连接串，仅支持 PostgreSQL。"""
    try:
        parts = urlsplit(url)
    except ValueError as exc:  # pragma: no cover - urlsplit 极少抛错
        raise ConfigError(f"无法解析连接串：{exc}") from exc

    scheme = (parts.scheme or "").lower()
    if not scheme:
        raise ConfigError("无法解析连接串：缺少 scheme（如 postgresql://）")
    if scheme not in ALLOWED_SCHEMES:
        raise ConfigError(
            f"暂不支持的数据库类型：{scheme}（本工具仅支持 PostgreSQL）"
        )

    try:
        port = parts.port
    except ValueError as exc:
        raise ConfigError(f"连接串端口号非法：{exc}") from exc

    database = parts.path.lstrip("/") or None
    return DbTarget(
        scheme=scheme,
        user=parts.username,
        password=parts.password,
        host=parts.hostname,
        port=port,
        database=database,
    )


def redact_url(url: str) -> str:
    """把连接串脱敏成可安全打印的形式（去掉口令）。"""
    try:
        parts = urlsplit(url)
    except ValueError:
        return "<无法解析的连接串>"
    if not parts.scheme:
        return "<无法解析的连接串>"

    netloc = parts.hostname or ""
    if parts.port:
        netloc = f"{netloc}:{parts.port}"
    if parts.username:
        netloc = f"{parts.username}@{netloc}"
    return f"{parts.scheme}://{netloc}{parts.path}"
