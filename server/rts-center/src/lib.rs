//! rts-center：搬运工平台的服务中心。
//!
//! 对外提供 HTTP 接口，基址 `/api/v1`：
//!
//! | 方法 | 路径 | 说明 |
//! |------|------|------|
//! | GET | `/api/v1/auth/captcha` | 获取图形验证码 |
//! | POST | `/api/v1/auth/login` | 登录（验证码 → 口令 → 锁定） |
//! | POST | `/api/v1/auth/logout` | 登出 |
//! | GET | `/api/v1/health` | 健康检查 |
//!
//! 接口契约见 `sdlc/20261025_登录验证码/spec.md`。

pub mod auth;
pub mod captcha;
pub mod config;
pub mod error;
pub mod routes;
pub mod state;

#[cfg(test)]
mod tests;
