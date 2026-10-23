//! SSH 连接、认证与指令执行。

use std::fmt;
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use russh::client::{self, Handle};
use russh::keys::{PublicKey, PublicKeyOrCertificate};
use russh::{ChannelMsg, Disconnect};

use crate::config::Config;
use crate::hostkey::{HostKeyStore, Status};

/// 连接与握手超时。
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// 单条指令默认超时。
pub const DEFAULT_COMMAND_TIMEOUT: Duration = Duration::from_secs(300);

/// 收到退出码后，等待远端关闭通道的宽限时间。
const CLOSE_GRACE: Duration = Duration::from_secs(2);

/// 环境变量：覆盖单条指令超时（秒）。
pub const COMMAND_TIMEOUT_ENV: &str = "RTS_COMMAND_TIMEOUT";

/// 错误类别，决定进程退出码。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// 连接失败（网络不可达、超时、握手失败）。
    Connect,
    /// 认证失败。
    Auth,
    /// 主机密钥校验失败。
    HostKey,
    /// 执行阶段失败。
    Exec,
}

/// 会话错误。
#[derive(Debug)]
pub struct SessionError {
    /// 错误类别。
    pub kind: ErrorKind,
    /// 面向用户的错误描述。
    pub message: String,
}

impl SessionError {
    fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        SessionError {
            kind,
            message: message.into(),
        }
    }

    /// 对应的进程退出码。
    pub fn exit_code(&self) -> i32 {
        match self.kind {
            ErrorKind::Connect => 2,
            ErrorKind::Auth => 3,
            ErrorKind::HostKey => 4,
            ErrorKind::Exec => 5,
        }
    }

    /// 是否为致命错误（连接已不可用，应停止后续指令）。
    pub fn is_fatal(&self) -> bool {
        matches!(self.kind, ErrorKind::Connect | ErrorKind::Exec)
    }
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for SessionError {}

/// 单条指令的执行结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// 远端返回的退出码；远端未提供时为 `None`。
    pub exit_code: Option<u32>,
    /// 是否因超时而中断。
    pub timed_out: bool,
}

/// 已建立的 SSH 会话。
pub struct Session {
    handle: Handle<HostKeyHandler>,
}

impl Session {
    /// 执行一条远端指令。
    ///
    /// stdout 数据流式写入 `out`，stderr 数据流式写入 `err`。
    pub async fn exec(
        &mut self,
        command: &str,
        out: &mut dyn Write,
        err: &mut dyn Write,
    ) -> Result<Outcome, SessionError> {
        let timeout = command_timeout();
        match tokio::time::timeout(timeout, self.exec_inner(command, out, err)).await {
            Ok(result) => result,
            Err(_) => Ok(Outcome {
                exit_code: None,
                timed_out: true,
            }),
        }
    }

    async fn exec_inner(
        &mut self,
        command: &str,
        out: &mut dyn Write,
        err: &mut dyn Write,
    ) -> Result<Outcome, SessionError> {
        let mut channel =
            self.handle.channel_open_session().await.map_err(|e| {
                SessionError::new(ErrorKind::Exec, format!("打开远端通道失败: {e}"))
            })?;

        channel
            .exec(true, command)
            .await
            .map_err(|e| SessionError::new(ErrorKind::Exec, format!("下发指令失败: {e}")))?;

        let mut exit_code = None;
        loop {
            // 收到退出码后，给远端一点时间关闭通道；远端不关闭时也不再无限等待。
            let msg = if exit_code.is_some() {
                tokio::time::timeout(CLOSE_GRACE, channel.wait())
                    .await
                    .unwrap_or_default()
            } else {
                channel.wait().await
            };

            let Some(msg) = msg else { break };

            match msg {
                ChannelMsg::Data { data } => {
                    write_stream(out, &data)?;
                }
                ChannelMsg::ExtendedData { data, ext: 1 } => {
                    write_stream(err, &data)?;
                }
                ChannelMsg::ExitStatus { exit_status } => exit_code = Some(exit_status),
                _ => {}
            }
        }

        Ok(Outcome {
            exit_code,
            timed_out: false,
        })
    }

    /// 断开连接。
    pub async fn disconnect(&mut self) {
        let _ = self
            .handle
            .disconnect(Disconnect::ByApplication, "", "zh-CN")
            .await;
    }
}

/// 将一段输出写入目标流。
///
/// 下游管道被关闭（如 `| head`）时静默退出，不视为错误。
fn write_stream(target: &mut dyn Write, data: &[u8]) -> Result<(), SessionError> {
    match target.write_all(data).and_then(|_| target.flush()) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {
            std::process::exit(0);
        }
        Err(e) => Err(SessionError::new(
            ErrorKind::Exec,
            format!("回显输出失败: {e}"),
        )),
    }
}

/// 建立 SSH 连接并完成密码认证。
///
/// 返回会话与主机密钥校验结果。
pub async fn connect(
    config: &Config,
    password: &str,
    store: HostKeyStore,
) -> Result<(Session, Status), SessionError> {
    let client_config = Arc::new(client::Config {
        inactivity_timeout: Some(Duration::from_secs(30)),
        keepalive_interval: Some(Duration::from_secs(15)),
        keepalive_max: 3,
        nodelay: true,
        ..Default::default()
    });

    let verdict: Arc<Mutex<Option<Result<Status, String>>>> = Arc::new(Mutex::new(None));
    let handler = HostKeyHandler {
        host: config.host.clone(),
        port: config.port,
        store,
        verdict: verdict.clone(),
    };

    let addr = (config.host.as_str(), config.port);
    let mut handle = tokio::time::timeout(
        CONNECT_TIMEOUT,
        client::connect(client_config, addr, handler),
    )
    .await
    .map_err(|_| {
        SessionError::new(
            ErrorKind::Connect,
            format!("连接 {}:{} 超时", config.host, config.port),
        )
    })?
    .map_err(|e| {
        SessionError::new(
            ErrorKind::Connect,
            format!("连接 {}:{} 失败: {e}", config.host, config.port),
        )
    })?;

    let status = match verdict.lock().unwrap().take() {
        Some(Ok(status)) => status,
        Some(Err(message)) => return Err(SessionError::new(ErrorKind::HostKey, message)),
        None => {
            return Err(SessionError::new(
                ErrorKind::HostKey,
                "服务端未提供主机密钥，无法校验".to_string(),
            ));
        }
    };

    let auth = handle
        .authenticate_password(config.user.clone(), password.to_string())
        .await
        .map_err(|e| SessionError::new(ErrorKind::Auth, format!("认证过程出错: {e}")))?;

    if !auth.success() {
        return Err(SessionError::new(
            ErrorKind::Auth,
            format!("认证失败: 用户 {} 的用户名或密码不正确", config.user),
        ));
    }

    Ok((Session { handle }, status))
}

/// 单条指令的超时时长，可由 `RTS_COMMAND_TIMEOUT`（秒）覆盖。
fn command_timeout() -> Duration {
    std::env::var(COMMAND_TIMEOUT_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .filter(|secs| *secs > 0)
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_COMMAND_TIMEOUT)
}

/// 客户端事件处理器：负责主机密钥校验。
struct HostKeyHandler {
    host: String,
    port: u16,
    store: HostKeyStore,
    verdict: Arc<Mutex<Option<Result<Status, String>>>>,
}

impl client::Handler for HostKeyHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let key = match server_public_key {
            PublicKeyOrCertificate::PublicKey { key, .. } => key.clone(),
            PublicKeyOrCertificate::Certificate(cert) => PublicKey::from(cert.public_key().clone()),
        };

        let result = self
            .store
            .verify(&self.host, self.port, &key)
            .map_err(|e| e.to_string());
        let accepted = result.is_ok();
        *self.verdict.lock().unwrap() = Some(result);
        Ok(accepted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;
    use std::path::PathBuf;

    use russh::ChannelId;
    use russh::server::{Auth, Handler as ServerHandler, Server as _, Session as ServerSession};
    use tokio::net::TcpListener;

    use crate::config::Protocol;
    use crate::hostkey::Status;

    /// 测试用服务端：接受 tester/secret，模拟几条固定指令。
    struct TestServer;

    impl russh::server::Server for TestServer {
        type Handler = TestServer;

        fn new_client(&mut self, _peer_addr: Option<SocketAddr>) -> Self::Handler {
            TestServer
        }
    }

    impl ServerHandler for TestServer {
        type Error = russh::Error;

        async fn channel_open_session(
            &mut self,
            _channel: russh::Channel<russh::server::Msg>,
            reply: russh::server::ChannelOpenHandle,
            _session: &mut ServerSession,
        ) -> Result<(), Self::Error> {
            reply.accept().await;
            Ok(())
        }

        async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
            if user == "tester" && password == "secret" {
                Ok(Auth::Accept)
            } else {
                Ok(Auth::reject())
            }
        }

        async fn exec_request(
            &mut self,
            channel: ChannelId,
            data: &[u8],
            session: &mut ServerSession,
        ) -> Result<(), Self::Error> {
            let command = String::from_utf8_lossy(data).to_string();
            session.channel_success(channel).ok();

            match command.as_str() {
                "boom" => {
                    session.extended_data(channel, 1, b"oops\n".to_vec()).ok();
                    session.exit_status_request(channel, 3).ok();
                }
                _ => {
                    session
                        .data(channel, format!("ran: {command}\n").into_bytes())
                        .ok();
                    session.exit_status_request(channel, 0).ok();
                }
            }
            session.eof(channel).ok();
            session.close(channel).ok();
            Ok(())
        }
    }

    async fn start_server() -> (SocketAddr, tokio::task::JoinHandle<()>) {
        let key =
            russh::keys::PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519)
                .expect("生成服务端主机密钥失败");
        let config = Arc::new(russh::server::Config {
            inactivity_timeout: Some(Duration::from_secs(60)),
            auth_rejection_time: Duration::from_millis(10),
            auth_rejection_time_initial: Some(Duration::from_millis(0)),
            keys: vec![key],
            ..Default::default()
        });

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            let mut server = TestServer;
            let _ = server.run_on_socket(config, &listener).await;
        });
        (addr, handle)
    }

    fn config_for(addr: SocketAddr) -> Config {
        Config {
            protocol: Protocol::Sftp,
            host: addr.ip().to_string(),
            port: addr.port(),
            user: "tester".to_string(),
            password: Some("secret".to_string()),
            commands: Vec::new(),
        }
    }

    fn temp_known_hosts() -> (PathBuf, HostKeyStore) {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "r_connector_e2e_known_hosts_{}_{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_file(&path);
        (path.clone(), HostKeyStore::with_path(path))
    }

    #[tokio::test]
    async fn executes_commands_and_reports_exit_codes() {
        let (addr, server) = start_server().await;
        let (known_hosts_path, store) = temp_known_hosts();
        let config = config_for(addr);

        let (mut session, status) = connect(&config, "secret", store.clone())
            .await
            .expect("连接应成功");
        assert!(matches!(status, Status::FirstTime { .. }));

        let mut out = Vec::new();
        let mut err = Vec::new();
        let outcome = session.exec("uname -a", &mut out, &mut err).await.unwrap();
        assert_eq!(outcome.exit_code, Some(0));
        assert_eq!(String::from_utf8(out).unwrap(), "ran: uname -a\n");
        assert!(err.is_empty());

        let mut out = Vec::new();
        let mut err = Vec::new();
        let outcome = session.exec("boom", &mut out, &mut err).await.unwrap();
        assert_eq!(outcome.exit_code, Some(3));
        assert!(out.is_empty());
        assert_eq!(String::from_utf8(err).unwrap(), "oops\n");

        session.disconnect().await;
        server.abort();
        let _ = std::fs::remove_file(known_hosts_path);
    }

    #[tokio::test]
    async fn second_connection_reuses_recorded_host_key() {
        let (addr, server) = start_server().await;
        let (known_hosts_path, store) = temp_known_hosts();
        let config = config_for(addr);

        let (mut session, _) = connect(&config, "secret", store.clone()).await.unwrap();
        session.disconnect().await;

        let (_, status) = connect(&config, "secret", store).await.unwrap();
        assert_eq!(status, Status::Trusted);

        server.abort();
        let _ = std::fs::remove_file(known_hosts_path);
    }

    #[tokio::test]
    async fn wrong_password_is_reported_as_auth_failure() {
        let (addr, server) = start_server().await;
        let (known_hosts_path, store) = temp_known_hosts();
        let config = config_for(addr);

        let err = match connect(&config, "wrong", store).await {
            Ok(_) => panic!("错误密码不应连接成功"),
            Err(e) => e,
        };
        assert_eq!(err.kind, ErrorKind::Auth);

        server.abort();
        let _ = std::fs::remove_file(known_hosts_path);
    }
}
