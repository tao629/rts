//! 接口级单元测试。
//!
//! 用 axum 的 `oneshot` 直接驱动路由，不占真实端口。
//!
//! 验证码答案不会出现在响应里，所以测试通过 [`AppState`] 直接向验证码存储
//! 写入一个**已知答案**的验证码，再拿它去调登录接口。
//!
//! 账号来源用**内存态** `AccountStore`：接口级测试不依赖真实数据库，
//! 生产的查库路径由联调阶段（verify.md）单独验证。
//!
//! 测试用的账号口令与签名密钥都在**运行时随机生成**，仓库里不出现任何凭据字面量。

use std::collections::HashMap;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{Value, json};
use tower::ServiceExt;

use crate::auth::users::{AccountSpec, AccountStatus, AccountStore, MemoryAccountStore};
use crate::config::{Config, ENV_JWT_SECRET, RANDOM_SECRET_BYTES, resolve_jwt_secret};
use crate::routes::build_router;
use crate::state::AppState;

/// 运行时随机口令（24 字节十六进制）。
fn random_password() -> String {
    crate::config::random_secret_with_len(24)
}

/// 测试用的两个账号：口令运行时随机生成。
fn test_specs() -> Vec<AccountSpec> {
    vec![
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
    ]
}

/// 测试应用：路由 + 状态 + 账号明文口令对照表。
struct TestApp {
    router: Router,
    state: Arc<AppState>,
    /// 用户名 → 明文口令（只用于构造请求，不进被测代码）。
    passwords: HashMap<String, String>,
}

impl TestApp {
    /// 用默认配置构造。
    fn build() -> Self {
        Self::with_config(Config::default())
    }

    /// 用指定配置构造。
    fn with_config(config: Config) -> Self {
        let specs = test_specs();
        let passwords = specs
            .iter()
            .map(|spec| (spec.username.clone(), spec.password.clone()))
            .collect();
        let store = AccountStore::Memory(MemoryAccountStore::from_specs(specs));
        let state = Arc::new(AppState::new(config, store));
        let router = build_router(state.clone());
        Self {
            router,
            state,
            passwords,
        }
    }

    /// 取某个账号的明文口令。
    fn password(&self, username: &str) -> String {
        self.passwords
            .get(username)
            .cloned()
            .unwrap_or_else(random_password)
    }

    /// 登录并取回令牌。
    async fn login_token(&self, username: &str) -> String {
        let captcha_id = issue_captcha(&self.state, "AB3D").await;
        let request = login_request(
            username,
            &self.password(username),
            false,
            &captcha_id,
            "AB3D",
        );
        let (status, body) = call(&self.router, request).await;
        assert_eq!(status, StatusCode::OK, "登录应当成功，实际响应：{body}");
        body.get("token")
            .and_then(Value::as_str)
            .expect("响应里应当有 token")
            .to_owned()
    }

    /// 把某个账号停用 / 启用。
    async fn set_status(&self, username: &str, status: AccountStatus) {
        self.state
            .store
            .set_status(username, status)
            .await
            .expect("修改账号状态应当成功");
    }
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

/// 构造带 Bearer 令牌的 GET 请求。
fn authed_get(uri: &str, token: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .expect("构造请求失败")
}

/// 构造带 Bearer 令牌的 JSON 请求。
fn authed_json(method: &str, uri: &str, token: &str, payload: Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .body(Body::from(payload.to_string()))
        .expect("构造请求失败")
}

/// 取出响应中的错误码。
fn error_code(body: &Value) -> &str {
    body.get("code").and_then(Value::as_str).unwrap_or_default()
}

/// A1：验证码接口返回三项字段，图片是 base64 data URI。
#[tokio::test]
async fn captcha_endpoint_returns_three_fields_with_data_uri() {
    let app = TestApp::build();
    let request = Request::builder()
        .method("GET")
        .uri("/api/v1/auth/captcha")
        .body(Body::empty())
        .expect("构造请求失败");
    let (status, body) = call(&app.router, request).await;

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
    let app = TestApp::build();
    let request = Request::builder()
        .method("GET")
        .uri("/api/v1/auth/captcha")
        .body(Body::empty())
        .expect("构造请求失败");
    let response = app
        .router
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
    let app = TestApp::build();
    let password = app.password("admin");
    let captcha_id = issue_captcha(&app.state, "AB3D").await;

    let request = login_request("admin", &password, true, &captcha_id, "AB3D");
    let (status, body) = call(&app.router, request).await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.get("token").and_then(Value::as_str).is_some());
    assert_eq!(
        body.get("tokenType").and_then(Value::as_str),
        Some("Bearer")
    );
    assert!(body.get("expiresAt").and_then(Value::as_str).is_some());
    let user = body.get("user").expect("缺少 user");
    assert_eq!(user.get("id").and_then(Value::as_str), Some("1"));
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
    let app = TestApp::build();
    let password = app.password("admin");
    let secret = app.state.config.jwt_secret.clone();
    let captcha_id = issue_captcha(&app.state, "AB3D").await;
    let request = login_request("admin", &password, false, &captcha_id, "AB3D");
    let (status, body) = call(&app.router, request).await;
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
    let app = TestApp::build();
    let password = app.password("admin");
    let captcha_id = issue_captcha(&app.state, "AB3D").await;

    let request = login_request("admin", &password, false, &captcha_id, "ZZZZ");
    let (status, body) = call(&app.router, request).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error_code(&body), "INVALID_CAPTCHA");
}

/// A2：验证码缺失 → 400 INVALID_CAPTCHA。
#[tokio::test]
async fn login_without_captcha_returns_invalid_captcha() {
    let app = TestApp::build();

    let request = login_request("admin", &random_password(), false, "", "");
    let (status, body) = call(&app.router, request).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error_code(&body), "INVALID_CAPTCHA");
}

/// A2：验证码字段整个不传（JSON 里没有 captchaId / captchaCode）→ 400 INVALID_CAPTCHA。
#[tokio::test]
async fn login_with_missing_captcha_fields_returns_invalid_captcha() {
    let app = TestApp::build();

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
    let (status, body) = call(&app.router, request).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error_code(&body), "INVALID_CAPTCHA");
}

/// A3：同一 captchaId 第二次使用 → 400 INVALID_CAPTCHA（一次性）。
#[tokio::test]
async fn captcha_cannot_be_used_twice() {
    let app = TestApp::build();
    let password = app.password("admin");
    let captcha_id = issue_captcha(&app.state, "AB3D").await;

    let first = login_request("admin", &password, false, &captcha_id, "AB3D");
    let (status, _) = call(&app.router, first).await;
    assert_eq!(status, StatusCode::OK);

    let second = login_request("admin", &password, false, &captcha_id, "AB3D");
    let (status, body) = call(&app.router, second).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error_code(&body), "INVALID_CAPTCHA");
}

/// A4：过期验证码 → 400 INVALID_CAPTCHA。
///
/// 用 TTL = 0 的配置构造存储，写入即过期，避免测试里真的等待。
#[tokio::test]
async fn expired_captcha_returns_invalid_captcha() {
    let config = Config {
        captcha_ttl_secs: 0,
        ..Config::default()
    };
    let app = TestApp::with_config(config);
    let password = app.password("admin");
    let captcha_id = issue_captcha(&app.state, "AB3D").await;

    let request = login_request("admin", &password, false, &captcha_id, "AB3D");
    let (status, body) = call(&app.router, request).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error_code(&body), "INVALID_CAPTCHA");
}

/// 验证码大小写不敏感。
#[tokio::test]
async fn captcha_comparison_is_case_insensitive() {
    let app = TestApp::build();
    let password = app.password("admin");
    let captcha_id = issue_captcha(&app.state, "AB3D").await;

    let request = login_request("admin", &password, false, &captcha_id, "ab3d");
    let (status, body) = call(&app.router, request).await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.get("token").and_then(Value::as_str).is_some());
}

/// A6：口令错误 + 验证码正确 → 401 INVALID_CREDENTIALS，且该验证码已失效。
#[tokio::test]
async fn wrong_password_returns_401_and_consumes_captcha() {
    let app = TestApp::build();
    let password = app.password("admin");
    let captcha_id = issue_captcha(&app.state, "AB3D").await;

    let request = login_request("admin", &random_password(), false, &captcha_id, "AB3D");
    let (status, body) = call(&app.router, request).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&body), "INVALID_CREDENTIALS");

    // 验证码已被消费，换成正确口令再来一次也应当失败。
    let retry = login_request("admin", &password, false, &captcha_id, "AB3D");
    let (status, body) = call(&app.router, retry).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error_code(&body), "INVALID_CAPTCHA");
}

/// 验证码先于口令：验证码错时即使口令正确也失败，且不泄露账号是否存在。
#[tokio::test]
async fn captcha_is_checked_before_credentials() {
    let app = TestApp::build();
    let password = app.password("admin");
    let captcha_id = issue_captcha(&app.state, "AB3D").await;

    let request = login_request("no-such-user", &password, false, &captcha_id, "ZZZZ");
    let (status, body) = call(&app.router, request).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error_code(&body), "INVALID_CAPTCHA");
}

/// 账号不存在与口令错误返回同样的错误码（避免账号枚举）。
#[tokio::test]
async fn unknown_user_returns_same_error_as_wrong_password() {
    let app = TestApp::build();

    let captcha_id = issue_captcha(&app.state, "AB3D").await;
    let unknown = login_request(
        "no-such-user",
        &random_password(),
        false,
        &captcha_id,
        "AB3D",
    );
    let (status_unknown, body_unknown) = call(&app.router, unknown).await;

    let captcha_id = issue_captcha(&app.state, "AB3D").await;
    let wrong = login_request("admin", &random_password(), false, &captcha_id, "AB3D");
    let (status_wrong, body_wrong) = call(&app.router, wrong).await;

    assert_eq!(status_unknown, StatusCode::UNAUTHORIZED);
    assert_eq!(status_wrong, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&body_unknown), "INVALID_CREDENTIALS");
    assert_eq!(error_code(&body_unknown), error_code(&body_wrong));
}

/// 账号表为空时，登录一律失败（而不是放行）。
#[tokio::test]
async fn login_fails_when_account_table_is_empty() {
    let state = Arc::new(AppState::new(
        Config::default(),
        AccountStore::Memory(MemoryAccountStore::default()),
    ));
    let router = build_router(state.clone());
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
    let config = Config {
        login_max_failures: 3,
        ..Config::default()
    };
    let app = TestApp::with_config(config);
    let password = app.password("admin");

    // 前三次失败：401。达到阈值的那一次仍然算"口令错误"。
    for _ in 0..3 {
        let captcha_id = issue_captcha(&app.state, "AB3D").await;
        let request = login_request("admin", &random_password(), false, &captcha_id, "AB3D");
        let (status, body) = call(&app.router, request).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(error_code(&body), "INVALID_CREDENTIALS");
    }

    // 第四次尝试：账号已锁定。
    let captcha_id = issue_captcha(&app.state, "AB3D").await;
    let request = login_request("admin", &random_password(), false, &captcha_id, "AB3D");
    let (status, body) = call(&app.router, request).await;
    assert_eq!(status, StatusCode::LOCKED);
    assert_eq!(error_code(&body), "ACCOUNT_LOCKED");

    // 锁定期内即便口令正确也返回 423。
    let captcha_id = issue_captcha(&app.state, "AB3D").await;
    let request = login_request("admin", &password, false, &captcha_id, "AB3D");
    let (status, body) = call(&app.router, request).await;
    assert_eq!(status, StatusCode::LOCKED);
    assert_eq!(error_code(&body), "ACCOUNT_LOCKED");
}

/// 登录成功会清零失败计数。
#[tokio::test]
async fn successful_login_resets_failure_counter() {
    let config = Config {
        login_max_failures: 3,
        ..Config::default()
    };
    let app = TestApp::with_config(config);
    let password = app.password("admin");

    for _ in 0..2 {
        let captcha_id = issue_captcha(&app.state, "AB3D").await;
        let request = login_request("admin", &random_password(), false, &captcha_id, "AB3D");
        let (status, _) = call(&app.router, request).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    let captcha_id = issue_captcha(&app.state, "AB3D").await;
    let request = login_request("admin", &password, false, &captcha_id, "AB3D");
    let (status, _) = call(&app.router, request).await;
    assert_eq!(status, StatusCode::OK);

    // 计数已清零：再错两次也不会锁定。
    for _ in 0..2 {
        let captcha_id = issue_captcha(&app.state, "AB3D").await;
        let request = login_request("admin", &random_password(), false, &captcha_id, "AB3D");
        let (status, _) = call(&app.router, request).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
}

/// 用户名为空 → 400 VALIDATION_ERROR。
#[tokio::test]
async fn empty_username_returns_validation_error() {
    let app = TestApp::build();
    let password = app.password("admin");
    let captcha_id = issue_captcha(&app.state, "AB3D").await;

    let request = login_request("   ", &password, false, &captcha_id, "AB3D");
    let (status, body) = call(&app.router, request).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error_code(&body), "VALIDATION_ERROR");
}

/// 请求体非法 → 400 VALIDATION_ERROR。
#[tokio::test]
async fn malformed_body_returns_validation_error() {
    let app = TestApp::build();

    let request = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/login")
        .header("content-type", "application/json")
        .body(Body::from("{not-json"))
        .expect("构造请求失败");
    let (status, body) = call(&app.router, request).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error_code(&body), "VALIDATION_ERROR");
}

/// 健康检查。
#[tokio::test]
async fn health_endpoint_reports_ok() {
    let app = TestApp::build();
    let request = Request::builder()
        .method("GET")
        .uri("/api/v1/health")
        .body(Body::empty())
        .expect("构造请求失败");
    let (status, body) = call(&app.router, request).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.get("status").and_then(Value::as_str), Some("ok"));
    assert!(body.get("time").and_then(Value::as_str).is_some());
}

/// 登出返回 204 空响应。
#[tokio::test]
async fn logout_returns_no_content() {
    let app = TestApp::build();
    let request = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/logout")
        .body(Body::empty())
        .expect("构造请求失败");
    let (status, body) = call(&app.router, request).await;

    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(body.is_null());
}

/// operator 账号同样可以登录。
#[tokio::test]
async fn operator_account_can_login() {
    let app = TestApp::build();
    let password = app.password("operator");
    let captcha_id = issue_captcha(&app.state, "AB3D").await;

    let request = login_request("operator", &password, false, &captcha_id, "AB3D");
    let (status, body) = call(&app.router, request).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body.get("user")
            .and_then(|user| user.get("username"))
            .and_then(Value::as_str),
        Some("operator")
    );
}

/// AC-08：停用账号登录 → 403 ACCOUNT_DISABLED。
#[tokio::test]
async fn disabled_account_cannot_login() {
    let app = TestApp::build();
    app.set_status("admin", AccountStatus::Disabled).await;
    let password = app.password("admin");
    let captcha_id = issue_captcha(&app.state, "AB3D").await;

    let request = login_request("admin", &password, false, &captcha_id, "AB3D");
    let (status, body) = call(&app.router, request).await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(error_code(&body), "ACCOUNT_DISABLED");
}

/// 停用账号的提示不能被用来枚举账号：口令错误时仍返回 INVALID_CREDENTIALS。
#[tokio::test]
async fn disabled_account_with_wrong_password_still_reports_invalid_credentials() {
    let app = TestApp::build();
    app.set_status("admin", AccountStatus::Disabled).await;
    let captcha_id = issue_captcha(&app.state, "AB3D").await;

    let request = login_request("admin", &random_password(), false, &captcha_id, "AB3D");
    let (status, body) = call(&app.router, request).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&body), "INVALID_CREDENTIALS");
}

/// AC-09：带有效令牌调 `/auth/me` → 200 返回用户信息。
#[tokio::test]
async fn me_endpoint_returns_user_info_for_valid_token() {
    let app = TestApp::build();
    let token = app.login_token("admin").await;

    let (status, body) = call(&app.router, authed_get("/api/v1/auth/me", &token)).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.get("username").and_then(Value::as_str), Some("admin"));
    assert_eq!(
        body.get("displayName").and_then(Value::as_str),
        Some("系统管理员")
    );
    assert_eq!(
        body.get("roles")
            .and_then(Value::as_array)
            .map(|roles| roles.len()),
        Some(1)
    );
}

/// AC-09：`/auth/me` 不带令牌 → 401 TOKEN_INVALID。
#[tokio::test]
async fn me_endpoint_without_token_returns_token_invalid() {
    let app = TestApp::build();
    let request = Request::builder()
        .method("GET")
        .uri("/api/v1/auth/me")
        .body(Body::empty())
        .expect("构造请求失败");

    let (status, body) = call(&app.router, request).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&body), "TOKEN_INVALID");
}

/// `/auth/me` 带一个伪造的令牌 → 401 TOKEN_INVALID。
#[tokio::test]
async fn me_endpoint_with_forged_token_returns_token_invalid() {
    let app = TestApp::build();
    let (status, body) = call(
        &app.router,
        authed_get("/api/v1/auth/me", "not-a-real-token"),
    )
    .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&body), "TOKEN_INVALID");
}

/// AC-10：停用账号的令牌调 `/auth/me` → 403 ACCOUNT_DISABLED（停用即时生效）。
#[tokio::test]
async fn me_endpoint_rejects_disabled_account() {
    let app = TestApp::build();
    let token = app.login_token("admin").await;
    app.set_status("admin", AccountStatus::Disabled).await;

    let (status, body) = call(&app.router, authed_get("/api/v1/auth/me", &token)).await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(error_code(&body), "ACCOUNT_DISABLED");
}

/// AC-11：非 super_admin 调管理接口 → 403 FORBIDDEN。
#[tokio::test]
async fn admin_endpoint_rejects_non_admin_role() {
    let app = TestApp::build();
    let token = app.login_token("operator").await;

    let (status, body) = call(&app.router, authed_get("/api/v1/admin/users", &token)).await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(error_code(&body), "FORBIDDEN");
}

/// 管理接口不带令牌 → 401 TOKEN_INVALID。
#[tokio::test]
async fn admin_endpoint_without_token_returns_token_invalid() {
    let app = TestApp::build();
    let request = Request::builder()
        .method("GET")
        .uri("/api/v1/admin/users")
        .body(Body::empty())
        .expect("构造请求失败");

    let (status, body) = call(&app.router, request).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&body), "TOKEN_INVALID");
}

/// AC-12：super_admin 新增账号 → 201，且落库的是 argon2id 哈希。
#[tokio::test]
async fn admin_can_create_user_with_argon2_hash() {
    let app = TestApp::build();
    let token = app.login_token("admin").await;
    let password = random_password();

    let request = authed_json(
        "POST",
        "/api/v1/admin/users",
        &token,
        json!({
            "username": "newcomer",
            "password": password,
            "displayName": "新同事",
            "roles": ["operator"],
        }),
    );
    let (status, body) = call(&app.router, request).await;

    assert_eq!(status, StatusCode::CREATED, "响应：{body}");
    assert_eq!(
        body.get("username").and_then(Value::as_str),
        Some("newcomer")
    );
    assert_eq!(body.get("status").and_then(Value::as_str), Some("active"));
    // 响应里绝不出现口令或口令哈希。
    assert!(!body.to_string().contains(&password));
    assert!(!body.to_string().contains("$argon2id$"));

    // 存储里是 argon2id PHC 哈希，不是明文。
    let stored = app
        .state
        .store
        .find_by_username("newcomer")
        .await
        .expect("查询应当成功")
        .expect("账号应当存在");
    assert!(stored.password_hash.starts_with("$argon2id$"));
    assert!(!stored.password_hash.contains(&password));
}

/// AC-13：用户名重复 → 409 USERNAME_TAKEN。
#[tokio::test]
async fn creating_duplicated_username_returns_conflict() {
    let app = TestApp::build();
    let token = app.login_token("admin").await;

    let request = authed_json(
        "POST",
        "/api/v1/admin/users",
        &token,
        json!({
            "username": "admin",
            "password": random_password(),
            "displayName": "重名的家伙",
        }),
    );
    let (status, body) = call(&app.router, request).await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&body), "USERNAME_TAKEN");
}

/// 新增账号的字段校验：口令太短、用户名非法 → 400 VALIDATION_ERROR。
#[tokio::test]
async fn creating_user_rejects_invalid_fields() {
    let app = TestApp::build();
    let token = app.login_token("admin").await;

    let too_short = authed_json(
        "POST",
        "/api/v1/admin/users",
        &token,
        json!({
            "username": "newcomer",
            "password": "short",
            "displayName": "新同事",
        }),
    );
    let (status, body) = call(&app.router, too_short).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error_code(&body), "VALIDATION_ERROR");

    let bad_username = authed_json(
        "POST",
        "/api/v1/admin/users",
        &token,
        json!({
            "username": "有 空格的用户名",
            "password": random_password(),
            "displayName": "新同事",
        }),
    );
    let (status, body) = call(&app.router, bad_username).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error_code(&body), "VALIDATION_ERROR");
}

/// 停用账号 → 该账号随即无法登录，且已签发的令牌也失效。
#[tokio::test]
async fn disabling_user_blocks_login_and_invalidates_token() {
    let app = TestApp::build();
    // 用超管的令牌去停用 operator；operator 自己签发的令牌也一并失效。
    let admin_token = app.login_token("admin").await;
    let operator_token = app.login_token("operator").await;
    let password = app.password("operator");

    let request = authed_json(
        "PATCH",
        "/api/v1/admin/users/operator/status",
        &admin_token,
        json!({ "status": "disabled" }),
    );
    let (status, body) = call(&app.router, request).await;
    assert_eq!(status, StatusCode::OK, "响应：{body}");
    assert_eq!(body.get("status").and_then(Value::as_str), Some("disabled"));

    // 令牌随即失效。
    let (status, body) = call(&app.router, authed_get("/api/v1/auth/me", &operator_token)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(error_code(&body), "ACCOUNT_DISABLED");

    // 登录也被拒。
    let captcha_id = issue_captcha(&app.state, "AB3D").await;
    let request = login_request("operator", &password, false, &captcha_id, "AB3D");
    let (status, body) = call(&app.router, request).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(error_code(&body), "ACCOUNT_DISABLED");
}

/// 修改不存在的账号状态 → 404 NOT_FOUND。
#[tokio::test]
async fn updating_status_of_missing_user_returns_not_found() {
    let app = TestApp::build();
    let token = app.login_token("admin").await;

    let request = authed_json(
        "PATCH",
        "/api/v1/admin/users/nobody/status",
        &token,
        json!({ "status": "disabled" }),
    );
    let (status, body) = call(&app.router, request).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error_code(&body), "NOT_FOUND");
}

/// 重置口令 → 旧口令失效、新口令可登录。
#[tokio::test]
async fn resetting_password_switches_the_credential() {
    let app = TestApp::build();
    let token = app.login_token("admin").await;
    let new_password = random_password();

    let request = authed_json(
        "POST",
        "/api/v1/admin/users/operator/password",
        &token,
        json!({ "password": new_password }),
    );
    let (status, _) = call(&app.router, request).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // 旧口令失效。
    let captcha_id = issue_captcha(&app.state, "AB3D").await;
    let request = login_request(
        "operator",
        &app.password("operator"),
        false,
        &captcha_id,
        "AB3D",
    );
    let (status, body) = call(&app.router, request).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&body), "INVALID_CREDENTIALS");

    // 新口令可用。
    let captcha_id = issue_captcha(&app.state, "AB3D").await;
    let request = login_request("operator", &new_password, false, &captcha_id, "AB3D");
    let (status, body) = call(&app.router, request).await;
    assert_eq!(status, StatusCode::OK, "响应：{body}");
}

/// 列出账号：不含口令哈希。
#[tokio::test]
async fn listing_users_never_exposes_password_hash() {
    let app = TestApp::build();
    let token = app.login_token("admin").await;

    let (status, body) = call(&app.router, authed_get("/api/v1/admin/users", &token)).await;

    assert_eq!(status, StatusCode::OK);
    let users = body
        .get("users")
        .and_then(Value::as_array)
        .expect("缺少 users");
    assert_eq!(users.len(), 2);
    assert!(!body.to_string().contains("$argon2id$"));
    assert!(!body.to_string().contains("passwordHash"));
}
