//! 路由装配。
//!
//! 接口契约见 `intents/20261031_ui-login-real-backend/spec.md` §3，服务基址为 `/api/v1`。

use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::routing::{get, patch, post};
use chrono::Utc;
use serde::Serialize;

use crate::admin;
use crate::auth;
use crate::state::AppState;

/// 健康检查响应体。
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    /// 服务状态，固定 `ok`。
    pub status: String,
    /// 服务端当前时间（ISO 8601 UTC）。
    pub time: String,
}

/// 构建路由。
pub fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/v1/auth/captcha", get(auth::captcha_handler))
        .route("/api/v1/auth/login", post(auth::login_handler))
        .route("/api/v1/auth/logout", post(auth::logout_handler))
        .route("/api/v1/auth/me", get(auth::me_handler))
        .route(
            "/api/v1/admin/users",
            post(admin::create_user_handler).get(admin::list_users_handler),
        )
        .route(
            "/api/v1/admin/users/{username}/status",
            patch(admin::update_status_handler),
        )
        .route(
            "/api/v1/admin/users/{username}/password",
            post(admin::reset_password_handler),
        )
        .route("/api/v1/health", get(health_handler))
        .with_state(state)
}

/// `GET /api/v1/health`：健康检查。
async fn health_handler() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_owned(),
        time: auth::token::format_iso8601(Utc::now()),
    })
}
