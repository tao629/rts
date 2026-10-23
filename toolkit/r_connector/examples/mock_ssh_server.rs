//! Mock SSH 服务端：用于在没有 sshd 的环境下验证 r_connector。
//!
//! 用法：
//!
//! ```text
//! cargo run --example mock_ssh_server -- 127.0.0.1:2222
//! ```
//!
//! 口令固定为 `secret`（用户名任意）。收到 exec 请求后**不执行任何本地命令**，
//! 只按固定规则回显：stdout 输出 `mock: <指令>`，指令以 `!` 开头时改为输出到
//! stderr 并返回退出码 3。
//!
//! 仅供本地验证使用，不要用于生产环境。

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use russh::ChannelId;
use russh::server::{Auth, Handler, Msg, Server};
use tokio::net::TcpListener;

struct MockServer;

impl russh::server::Server for MockServer {
    type Handler = MockServer;

    fn new_client(&mut self, _peer_addr: Option<SocketAddr>) -> Self::Handler {
        MockServer
    }
}

impl Handler for MockServer {
    type Error = russh::Error;

    async fn channel_open_session(
        &mut self,
        _channel: russh::Channel<Msg>,
        reply: russh::server::ChannelOpenHandle,
        _session: &mut russh::server::Session,
    ) -> Result<(), Self::Error> {
        reply.accept().await;
        Ok(())
    }

    async fn auth_password(&mut self, _user: &str, password: &str) -> Result<Auth, Self::Error> {
        if password == "secret" {
            Ok(Auth::Accept)
        } else {
            Ok(Auth::reject())
        }
    }

    async fn exec_request(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut russh::server::Session,
    ) -> Result<(), Self::Error> {
        let command = String::from_utf8_lossy(data).to_string();
        session.channel_success(channel).ok();

        if let Some(failing) = command.strip_prefix('!') {
            session
                .extended_data(channel, 1, format!("mock stderr: {failing}\n").into_bytes())
                .ok();
            session.exit_status_request(channel, 3).ok();
        } else {
            session
                .data(channel, format!("mock: {command}\n").into_bytes())
                .ok();
            session.exit_status_request(channel, 0).ok();
        }

        session.eof(channel).ok();
        session.close(channel).ok();
        Ok(())
    }
}

#[tokio::main]
async fn main() {
    let addr: SocketAddr = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:2222".to_string())
        .parse()
        .expect("监听地址格式错误");

    let key = russh::keys::PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519)
        .expect("生成主机密钥失败");
    let config = Arc::new(russh::server::Config {
        inactivity_timeout: Some(Duration::from_secs(600)),
        auth_rejection_time: Duration::from_millis(100),
        auth_rejection_time_initial: Some(Duration::from_millis(0)),
        keys: vec![key],
        ..Default::default()
    });

    let listener = TcpListener::bind(addr).await.expect("监听失败");
    eprintln!("mock ssh server listening on {addr} (password: secret)");

    let mut server = MockServer;
    server
        .run_on_socket(config, &listener)
        .await
        .expect("服务端异常退出");
}
