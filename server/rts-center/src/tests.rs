//! 接口级单元测试。
//!
//! 用 axum 的 `oneshot` 直接驱动路由，不占真实端口。
//!
//! 验证码答案不会出现在响应里，所以测试通过 [`AppState`] 直接向验证码存储
//! 写入一个**已知答案**的验证码，再拿它去调登录接口。
//!
//! 测试用的账号口令与签名密钥都在**运行时随机生成**，仓库里不出现任何凭据字面量。

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{Value, json};
use tower::ServiceExt;

use crate::auth::users::AccountSpec;
use crate::config::{Config, ENV_JWT_SECRET, RANDOM_SECRET_BYTES, resolve_jwt_secret};
use crate::routes::build_router;
use crate::state::AppState;

/// 运行时随机口令（24 字节十六进制）。
fn random_password() -> String {
    crate::config::random_secret_with_len(24)
}

/// 测试用配置：注入两个账号，口令与签名密钥都在运行时随机生成。
fn test_config() -> Config {
    Config {
        auth_accounts: vec![
            AccountSpec {
                username: "admin".to_owned(),
                password: random_password(),
                display_name: "系统管理员".to_owned(),
                roles: vec!["super_admin".to_owned()],
            },
            AccountSpec {
                username: "operator".to_owned(),
                password: random_password(),
                display_name: "值班运维".to_owned(),
                roles: vec!["operator".to_owned()],
            },
        ],
        ..Config::default()
    }
}

/// 取出配置里某个账号的口令，用于构造登录请求。
fn password_of(config: &Config, username: &str) -> String {
    config
        .auth_accounts
        .iter()
        .find(|account| account.username == username)
        .expect("测试账号应当存在")
        .password
        .clone()
}

/// 构造应用与状态句柄。
fn build_app(config: Config) -> (Router, Arc<AppState>) {
    let state = Arc::new(AppState::new(config));
    let router = build_router(state.clone());
    (router, state)
}

/// 写入一个已知答案的验证码，返回 captchaId。
async fn issue_captcha(state: &AppState, answer: &str) -> String {
    let mut store = state.captcha.lock().await;
    store.issue(answer)
}

/// 发起请求并解析 JSON 响应。
async fn call(router: &Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = router
        .clone()
        .oneshot(request)
        .await
        .expect("请求处理不应失败");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("读取响应体失败");
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, value)
}

/// 构造登录请求。
fn login_request(
    username: &str,
    password: &str,
    remember: bool,
    captcha_id: &str,
    captcha_code: &str,
) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/api/v1/auth/login")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "username": username,
                "password": password,
                "remember": remember,
                "captchaId": captcha_id,
                "captchaCode": captcha_code,
            })
            .to_string(),
        ))
        .expect("构造请求失败")
}

/// 取出响应中的错误码。
fn error_code(body: &Value) -> &str {
    body.get("code").and_then(Value::as_str).unwrap_or_default()
}

/// A1：验证码接口返回三项字段，图片是 base64 data URI。
#[tokio::test]
async fn captcha_endpoint_returns_three_fields_with_data_uri() {
    let (router, _state) = build_app(test_config());
    let request = Request::builder()
        .method("GET")
        .uri("/api/v1/auth/captcha")
        .body(Body::empty())
        .expect("构造请求失败");
    let (status, body) = call(&router, request).await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.get("captchaId").and_then(Value::as_str).is_some());
    assert!(body.get("expiresAt").and_then(Value::as_str).is_some());
    let image = body
        .get("image")
        .and_then(Value::as_str)
        .expect("缺少 image");
    assert!(
        image.starts_with("data:image/svg+xml;base64,"),
        "image 前缀不符: {image}"
    );
    // 答案绝不出现在响应体里。
    assert!(!body.to_string().contains("captchaCode"));
    assert!(!body.to_string().contains("answer"));
}

/// A1 补充：响应头带 `Cache-Control: no-store`。
#[tokio::test]
async fn captcha_response_has_no_store_cache_control() {
    let (router, _state) = build_app(test_config());
    let request = Request::builder()
        .method("GET")
        .uri("/api/v1/auth/captcha")
        .body(Body::empty())
        .expect("构造请求失败");
    let response = router
        .clone()
        .oneshot(request)
        .await
        .expect("请求处理不应失败");
    assert_eq!(response.status(), StatusCode::OK);
    let cache_control = response
        .headers()
        .get("cache-control")
        .and_then(|value| value.to_str().ok())
        .expect("缺少 Cache-Control 头");
    assert_eq!(cache_control, "no-store");
}

/// A5：正确验证码 + 正确口令可以登录成功。
#[tokio::test]
async fn login_with_correct_captcha_and_password_succeeds() {
    let config = test_config();
    let password = password_of(&config, "admin");
    let (router, state) = build_app(config);
    let captcha_id = issue_captcha(&state, "AB3D").await;

    let request = login_request("admin", &password, true, &captcha_id, "AB3D");
    let (status, body) = call(&router, request).await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.get("token").and_then(Value::as_str).is_some());
    assert_eq!(
        body.get("tokenType").and_then(Value::as_str),
        Some("Bearer")
    );
    assert!(body.get("expiresAt").and_then(Value::as_str).is_some());
    let user = body.get("user").expect("缺少 user");
    assert_eq!(user.get("id").and_then(Value::as_str), Some("u-1001"));
    assert_eq!(user.get("username").and_then(Value::as_str), Some("admin"));
    assert_eq!(
        user.get("displayName").and_then(Value::as_str),
        Some("系统管理员")
    );
    assert_eq!(
        user.get("roles")
            .and_then(Value::as_array)
            .map(|roles| roles.len()),
        Some(1)
    );
}

/// A5 补充：令牌可以被服务端验签，且 remember=false 时有效期为 2 小时。
#[tokio::test]
async fn issued_token_is_verifiable_and_respects_remember_flag() {
    let config = test_config();
    let password = password_of(&config, "admin");
    let secret = config.jwt_secret.clone();
    let (router, state) = build_app(config);
    let captcha_id = issue_captcha(&state, "AB3D").await;
    let request = login_request("admin", &password, false, &captcha_id, "AB3D");
    let (status, body) = call(&router, request).await;
    assert_eq!(status, StatusCode::OK);

    let token = body
        .get("token")
        .and_then(Value::as_str)
        .expect("缺少 token");
    let claims = crate::auth::token::verify(&secret, token).expect("令牌应当可验证");
    assert_eq!(claims.username, "admin");

    let expires_at = chrono::DateTime::parse_from_rfc3339(
        body.get("expiresAt")
            .and_then(Value::as_str)
            .expect("缺少 expiresAt"),
    )
    .expect("expiresAt 应当是 ISO 8601");
    let hours = (expires_at.with_timezone(&chrono::Utc) - chrono::Utc::now()).num_seconds() / 3600;
    assert!(
        (1..=2).contains(&hours),
        "非 remember 令牌有效期应接近 2 小时，实际 {hours}"
    );
}

/// A2：验证码错误 → 400 INVALID_CAPTCHA。
#[tokio::test]
async fn login_with_wrong_captcha_returns_invalid_captcha() {
    let config = test_config();
    let password = password_of(&config, "admin");
    let (router, state) = build_app(config);
    let captcha_id = issue_captcha(&state, "AB3D").await;

    let request = login_request("admin", &password, false, &captcha_id, "ZZZZ");
    let (status, body) = call(&router, request).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error_code(&body), "INVALID_CAPTCHA");
}

/// A2：验证码缺失 → 400 INVALID_CAPTCHA。
#[tokio::test]
async fn login_without_captcha_returns_invalid_captcha() {
    let (router, _state) = build_app(test_config());

    let request = login_request("admin", &random_password(), false, "", "");
    let (status, body) = call(&router, request).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error_code(&body), "INVALID_CAPTCHA");
}

/// A2：验证码字段整个不传（JSON 里没有 captchaId / captchaCode）→ 400 INVALID_CAPTCHA。
#[tokio::test]
async fn login_with_missing_captcha_fields_returns_invalid_captcha() {
    let (router, _state) = build_app(test_config());

    let request = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/login")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "username": "admin",
                "password": random_password(),
                "remember": false
            })
            .to_string(),
        ))
        .expect("构造请求失败");
    let (status, body) = call(&router, request).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error_code(&body), "INVALID_CAPTCHA");
}

/// A3：同一 captchaId 第二次使用 → 400 INVALID_CAPTCHA（一次性）。
#[tokio::test]
async fn captcha_cannot_be_used_twice() {
    let config = test_config();
    let password = password_of(&config, "admin");
    let (router, state) = build_app(config);
    let captcha_id = issue_captcha(&state, "AB3D").await;

    let first = login_request("admin", &password, false, &captcha_id, "AB3D");
    let (status, _) = call(&router, first).await;
    assert_eq!(status, StatusCode::OK);

    let second = login_request("admin", &password, false, &captcha_id, "AB3D");
    let (status, body) = call(&router, second).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error_code(&body), "INVALID_CAPTCHA");
}

/// A4：过期验证码 → 400 INVALID_CAPTCHA。
///
/// 用 TTL = 0 的配置构造存储，写入即过期，避免测试里真的等待。
#[tokio::test]
async fn expired_captcha_returns_invalid_captcha() {
    let mut config = test_config();
    config.captcha_ttl_secs = 0;
    let password = password_of(&config, "admin");
    let (router, state) = build_app(config);
    let captcha_id = issue_captcha(&state, "AB3D").await;

    let request = login_request("admin", &password, false, &captcha_id, "AB3D");
    let (status, body) = call(&router, request).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error_code(&body), "INVALID_CAPTCHA");
}

/// 验证码大小写不敏感。
#[tokio::test]
async fn captcha_comparison_is_case_insensitive() {
    let config = test_config();
    let password = password_of(&config, "admin");
    let (router, state) = build_app(config);
    let captcha_id = issue_captcha(&state, "AB3D").await;

    let request = login_request("admin", &password, false, &captcha_id, "ab3d");
    let (status, body) = call(&router, request).await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.get("token").and_then(Value::as_str).is_some());
}

/// A6：口令错误 + 验证码正确 → 401 INVALID_CREDENTIALS，且该验证码已失效。
#[tokio::test]
async fn wrong_password_returns_401_and_consumes_captcha() {
    let config = test_config();
    // 口令要在注入账号表被搬走之前取出来（运行期内存里只剩哈希）。
    let password = password_of(&config, "admin");
    let (router, state) = build_app(config);
    let captcha_id = issue_captcha(&state, "AB3D").await;

    let request = login_request("admin", &random_password(), false, &captcha_id, "AB3D");
    let (status, body) = call(&router, request).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&body), "INVALID_CREDENTIALS");

    // 验证码已被消费，换成正确口令再来一次也应当失败。
    let retry = login_request("admin", &password, false, &captcha_id, "AB3D");
    let (status, body) = call(&router, retry).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error_code(&body), "INVALID_CAPTCHA");
}

/// 验证码先于口令：验证码错时即使口令正确也失败，且不泄露账号是否存在。
#[tokio::test]
async fn captcha_is_checked_before_credentials() {
    let config = test_config();
    let password = password_of(&config, "admin");
    let (router, state) = build_app(config);
    let captcha_id = issue_captcha(&state, "AB3D").await;

    let request = login_request("no-such-user", &password, false, &captcha_id, "ZZZZ");
    let (status, body) = call(&router, request).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error_code(&body), "INVALID_CAPTCHA");
}

/// 账号不存在与口令错误返回同样的错误码（避免账号枚举）。
#[tokio::test]
async fn unknown_user_returns_same_error_as_wrong_password() {
    let config = test_config();
    let (router, state) = build_app(config);

    let captcha_id = issue_captcha(&state, "AB3D").await;
    let unknown = login_request(
        "no-such-user",
        &random_password(),
        false,
        &captcha_id,
        "AB3D",
    );
    let (status_unknown, body_unknown) = call(&router, unknown).await;

    let captcha_id = issue_captcha(&state, "AB3D").await;
    let wrong = login_request("admin", &random_password(), false, &captcha_id, "AB3D");
    let (status_wrong, body_wrong) = call(&router, wrong).await;

    assert_eq!(status_unknown, StatusCode::UNAUTHORIZED);
    assert_eq!(status_wrong, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&body_unknown), "INVALID_CREDENTIALS");
    assert_eq!(error_code(&body_unknown), error_code(&body_wrong));
}

/// 未配置账号（`RTS_AUTH_USERS` 缺省）时服务照常启动，但所有登录都失败。
#[tokio::test]
async fn login_fails_when_no_account_is_configured() {
    // 缺省配置里没有任何账号，等价于"未配置 RTS_AUTH_USERS"。
    let config = Config::default();
    assert!(config.auth_accounts.is_empty());
    let (router, state) = build_app(config);
    let captcha_id = issue_captcha(&state, "AB3D").await;

    let request = login_request("admin", &random_password(), false, &captcha_id, "AB3D");
    let (status, body) = call(&router, request).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&body), "INVALID_CREDENTIALS");
}

/// 未配置签名密钥时初始化照常进行，密钥为随机值。
#[test]
fn jwt_secret_is_random_when_not_configured() {
    let (secret, ephemeral) = resolve_jwt_secret(None);
    assert!(ephemeral, "未配置时应标记为临时密钥");
    assert_eq!(secret.len(), RANDOM_SECRET_BYTES * 2);
    assert!(secret.chars().all(|ch| ch.is_ascii_hexdigit()));

    // 默认配置同样不携带固定密钥。
    let config = Config::default();
    assert!(config.jwt_secret_ephemeral);
    assert_eq!(config.jwt_secret.len(), RANDOM_SECRET_BYTES * 2);
    assert!(config.jwt_secret.chars().all(|ch| ch.is_ascii_hexdigit()));
    assert_ne!(config.jwt_secret, secret, "两次生成的随机密钥不应相同");

    // 若运行环境确实设置了该变量，则配置应当采用它。
    if let (Ok(raw), Ok(config)) = (std::env::var(ENV_JWT_SECRET), Config::from_env()) {
        assert_eq!(config.jwt_secret, raw.trim());
        assert!(!config.jwt_secret_ephemeral);
    }
}

/// A7：连续失败达到阈值后 → 423 ACCOUNT_LOCKED。
#[tokio::test]
async fn account_is_locked_after_max_failures() {
    let mut config = test_config();
    config.login_max_failures = 3;
    // 口令要在注入账号表被搬走之前取出来（运行期内存里只剩哈希）。
    let password = password_of(&config, "admin");
    let (router, state) = build_app(config);

    // 前三次失败：401。达到阈值的那一次仍然算"口令错误"。
    for _ in 0..3 {
        let captcha_id = issue_captcha(&state, "AB3D").await;
        let request = login_request("admin", &random_password(), false, &captcha_id, "AB3D");
        let (status, body) = call(&router, request).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(error_code(&body), "INVALID_CREDENTIALS");
    }

    // 第四次尝试：账号已锁定。
    let captcha_id = issue_captcha(&state, "AB3D").await;
    let request = login_request("admin", &random_password(), false, &captcha_id, "AB3D");
    let (status, body) = call(&router, request).await;
    assert_eq!(status, StatusCode::LOCKED);
    assert_eq!(error_code(&body), "ACCOUNT_LOCKED");

    // 锁定期内即便口令正确也返回 423。
    let captcha_id = issue_captcha(&state, "AB3D").await;
    let request = login_request("admin", &password, false, &captcha_id, "AB3D");
    let (status, body) = call(&router, request).await;
    assert_eq!(status, StatusCode::LOCKED);
    assert_eq!(error_code(&body), "ACCOUNT_LOCKED");
}

/// 登录成功会清零失败计数。
#[tokio::test]
async fn successful_login_resets_failure_counter() {
    let mut config = test_config();
    config.login_max_failures = 3;
    let password = password_of(&config, "admin");
    let (router, state) = build_app(config);

    for _ in 0..2 {
        let captcha_id = issue_captcha(&state, "AB3D").await;
        let request = login_request("admin", &random_password(), false, &captcha_id, "AB3D");
        let (status, _) = call(&router, request).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    let captcha_id = issue_captcha(&state, "AB3D").await;
    let request = login_request("admin", &password, false, &captcha_id, "AB3D");
    let (status, _) = call(&router, request).await;
    assert_eq!(status, StatusCode::OK);

    // 计数已清零：再错两次也不会锁定。
    for _ in 0..2 {
        let captcha_id = issue_captcha(&state, "AB3D").await;
        let request = login_request("admin", &random_password(), false, &captcha_id, "AB3D");
        let (status, _) = call(&router, request).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
}

/// 用户名为空 → 400 VALIDATION_ERROR。
#[tokio::test]
async fn empty_username_returns_validation_error() {
    let config = test_config();
    let password = password_of(&config, "admin");
    let (router, state) = build_app(config);
    let captcha_id = issue_captcha(&state, "AB3D").await;

    let request = login_request("   ", &password, false, &captcha_id, "AB3D");
    let (status, body) = call(&router, request).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error_code(&body), "VALIDATION_ERROR");
}

/// 请求体非法 → 400 VALIDATION_ERROR。
#[tokio::test]
async fn malformed_body_returns_validation_error() {
    let (router, _state) = build_app(test_config());

    let request = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/login")
        .header("content-type", "application/json")
        .body(Body::from("{not-json"))
        .expect("构造请求失败");
    let (status, body) = call(&router, request).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error_code(&body), "VALIDATION_ERROR");
}

/// 健康检查。
#[tokio::test]
async fn health_endpoint_reports_ok() {
    let (router, _state) = build_app(test_config());
    let request = Request::builder()
        .method("GET")
        .uri("/api/v1/health")
        .body(Body::empty())
        .expect("构造请求失败");
    let (status, body) = call(&router, request).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.get("status").and_then(Value::as_str), Some("ok"));
    assert!(body.get("time").and_then(Value::as_str).is_some());
}

/// 登出返回 204 空响应。
#[tokio::test]
async fn logout_returns_no_content() {
    let (router, _state) = build_app(test_config());
    let request = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/logout")
        .body(Body::empty())
        .expect("构造请求失败");
    let (status, body) = call(&router, request).await;

    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(body.is_null());
}

/// operator 账号同样可以登录。
#[tokio::test]
async fn operator_account_can_login() {
    let config = test_config();
    let password = password_of(&config, "operator");
    let (router, state) = build_app(config);
    let captcha_id = issue_captcha(&state, "AB3D").await;

    let request = login_request("operator", &password, false, &captcha_id, "AB3D");
    let (status, body) = call(&router, request).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body.get("user")
            .and_then(|user| user.get("username"))
            .and_then(Value::as_str),
        Some("operator")
    );
}
