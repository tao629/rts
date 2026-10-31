//! rts-center：搬运工平台的服务中心。
//!
//! 对外提供 HTTP 接口，基址 `/api/v1`：
//!
//! | 方法 | 路径 | 说明 |
//! |------|------|------|
//! | GET | `/api/v1/auth/captcha` | 获取图形验证码 |
//! | POST | `/api/v1/auth/login` | 登录（验证码 → 口令 → 锁定） |
//! | POST | `/api/v1/auth/logout` | 登出 |
//! | GET | `/api/v1/auth/me` | 用令牌换回当前用户信息 |
//! | POST | `/api/v1/admin/users` | 新增账号（需 `super_admin`） |
//! | GET | `/api/v1/admin/users` | 列出账号（需 `super_admin`） |
//! | PATCH | `/api/v1/admin/users/{username}/status` | 启用 / 停用账号（需 `super_admin`） |
//! | POST | `/api/v1/admin/users/{username}/password` | 重置口令（需 `super_admin`） |
//! | GET | `/api/v1/health` | 健康检查 |
//!
//! 接口契约见 `intents/20261031_ui-login-real-backend/spec.md`。

pub mod admin;
pub mod auth;
pub mod captcha;
pub mod config;
pub mod error;
pub mod routes;
pub mod state;

#[cfg(test)]
mod tests;
