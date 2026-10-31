-- rts 库结构基线
--
-- 本文件记录由本仓库管理的表结构（DDL）。迁移脚本在 scripts/database/sql/ 下，
-- 本文件与之保持一致，作为「仓库可见的 schema 真相」。
--
-- 迁移脚本：scripts/database/sql/002_auth_user.sql
-- 变更依据：intents/20261031_ui-login-real-backend/spec.md §2（CHG-001）

-- 登录账号表：口令仅存 argon2id PHC 哈希，不存明文；
-- roles 为角色名数组，status 取 active/disabled。
CREATE TABLE IF NOT EXISTS auth_user (
    id            bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    username      varchar(64)  NOT NULL,
    password_hash varchar(255) NOT NULL,
    display_name  varchar(64)  NOT NULL,
    roles         text[]       NOT NULL DEFAULT '{}',
    status        varchar(16)  NOT NULL DEFAULT 'active',
    created_at    timestamptz  NOT NULL DEFAULT now(),
    updated_at    timestamptz  NOT NULL DEFAULT now(),

    CONSTRAINT auth_user_username_format_chk
        CHECK (username ~ '^[A-Za-z0-9._-]{3,64}$'),
    CONSTRAINT auth_user_display_name_chk
        CHECK (length(btrim(display_name)) BETWEEN 1 AND 64),
    CONSTRAINT auth_user_status_chk
        CHECK (status IN ('active', 'disabled'))
);

COMMENT ON TABLE auth_user IS
    '登录账号表：口令仅存 argon2id PHC 哈希，不存明文；roles 为角色名数组，status 取 active/disabled';

COMMENT ON COLUMN auth_user.id IS
    '账号主键，由数据库生成';
COMMENT ON COLUMN auth_user.username IS
    '登录名，允许 [A-Za-z0-9._-]，长度 3~64；大小写不敏感唯一';
COMMENT ON COLUMN auth_user.password_hash IS
    'argon2id PHC 串（如 $argon2id$v=19$m=...$...），只写哈希，禁止写明文';
COMMENT ON COLUMN auth_user.display_name IS
    '显示名，1~64 字符，随登录响应与账号管理接口返回';
COMMENT ON COLUMN auth_user.roles IS
    '角色名数组；含 super_admin 才可调用账号管理接口；默认空数组';
COMMENT ON COLUMN auth_user.status IS
    '账号状态：active 可登录；disabled 登录被拒且已签发令牌失效';
COMMENT ON COLUMN auth_user.created_at IS
    '建号时间，只写一次';
COMMENT ON COLUMN auth_user.updated_at IS
    '最后变更时间（改状态、改口令时由服务端显式更新）';

-- 大小写不敏感唯一：登录名按不区分大小写处理，"Admin" 与 "admin" 视为同一个账号。
CREATE UNIQUE INDEX IF NOT EXISTS auth_user_username_lower_key
    ON auth_user (lower(username));
