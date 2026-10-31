---
name: dba-toolkit
description: 数据库表结构变更（建表/加列/改名/删列/删表）或批量数据操作时使用：备份前置、幂等迁移、COMMENT 注解。触发词：建表、迁移、加列、删表、DDL、schema 变更。
---

# 数据库DDL操作规范

## 操作前备份

读取 `docs/database/rts-schema.sql` 与本次操作涉及的表内容备份到 `db-schema.bak`文件中

## 操作执行约束

1. 所有动作都通过 `nv run --directory scripts/database/db_caller.py db_schema.sql` 执行
