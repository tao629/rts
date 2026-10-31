"""dbtool —— db_caller.py 的内部实现包。

模块划分：

- `config`     ：`.env` 查找与载入、连接串读取与脱敏
- `splitter`   ：SQL 脚本切分（状态机，支持 dollar-quoted 与嵌套块注释）
- `tables`     ：从脚本中提取受影响的表名
- `migrations` ：迁移记录表（`schema_migrations`）与幂等判定
- `backup`     ：执行前的表结构备份（pg_dump 优先，回退 pg_catalog 内省）
- `executor`   ：连接、事务、逐条执行与结果回显
- `output`     ：日志着色与表格渲染
"""

__all__ = [
    "backup",
    "config",
    "executor",
    "migrations",
    "output",
    "splitter",
    "tables",
]
