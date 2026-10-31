-- 示例脚本：建表、加字段、建索引、灌数据
--
-- 用法（在 scripts/database 目录下）：
--   uv run db_caller.py --dry-run sql/001_example.sql   # 先预演，确认切分结果
--   uv run db_caller.py sql/001_example.sql              # 真正执行
--
-- 注意：脚本里的分号只有出现在语句末尾时才是分隔符；
-- 字符串、注释（含嵌套块注释）、$$ … $$ 函数体里的分号都不会把语句切开。

CREATE TABLE IF NOT EXISTS example_tasks (
    id          bigserial PRIMARY KEY,
    name        text        NOT NULL,
    status      text        NOT NULL DEFAULT 'pending',
    created_at  timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_example_tasks_status ON example_tasks (status);

ALTER TABLE example_tasks ADD COLUMN IF NOT EXISTS finished_at timestamptz;

INSERT INTO example_tasks (name, status) VALUES ('示例任务', 'pending');

-- 查询回显：结果会以表格打印
SELECT id, name, status, created_at FROM example_tasks ORDER BY id;
