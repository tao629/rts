//! 主机密钥校验（TOFU：首次信任并记录指纹，之后严格比对）。
//!
//! 信任记录保存在 `~/.ssh/known_hosts`（与 OpenSSH 工具链共用同一份文件），
//! 端口非 22 时以 `[host]:port` 形式匹配与记录。

use std::fmt;
use std::path::{Path, PathBuf};

use russh::keys::HashAlg;
use russh::keys::PublicKey;
use russh::keys::known_hosts::{known_host_keys_path, learn_known_hosts_path};

/// 环境变量：指定 known_hosts 文件路径。
pub const KNOWN_HOSTS_ENV: &str = "RTS_KNOWN_HOSTS";

/// 主机密钥校验结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// 已在信任记录中且公钥一致。
    Trusted,
    /// 首次连接，已记录指纹。
    FirstTime {
        /// 主机密钥指纹。
        fingerprint: String,
    },
}

/// 主机密钥校验错误。
#[derive(Debug)]
pub enum HostKeyError {
    /// 已记录的公钥与本次不一致。
    Changed {
        /// 已记录的指纹列表。
        recorded: Vec<String>,
        /// 本次连接出示的指纹。
        presented: String,
    },
    /// 读写 known_hosts 失败。
    Io(std::io::Error),
    /// known_hosts 内容无法解析。
    Parse(String),
}

impl fmt::Display for HostKeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HostKeyError::Changed {
                recorded,
                presented,
            } => {
                write!(
                    f,
                    "主机密钥与已知记录不一致（已记录: {}，本次: {}）",
                    recorded.join(", "),
                    presented
                )
            }
            HostKeyError::Io(e) => write!(f, "访问 known_hosts 失败: {e}"),
            HostKeyError::Parse(e) => write!(f, "解析 known_hosts 失败: {e}"),
        }
    }
}

impl std::error::Error for HostKeyError {}

/// 主机密钥信任记录。
#[derive(Debug, Clone)]
pub struct HostKeyStore {
    path: PathBuf,
}

impl HostKeyStore {
    /// 使用默认路径 `~/.ssh/known_hosts`。
    ///
    /// 可用环境变量 `RTS_KNOWN_HOSTS` 指定其它路径。
    pub fn default_path() -> HostKeyStore {
        if let Some(path) = std::env::var_os(KNOWN_HOSTS_ENV) {
            return HostKeyStore::with_path(path);
        }
        let path = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".ssh")
            .join("known_hosts");
        HostKeyStore::with_path(path)
    }

    /// 使用指定路径。
    pub fn with_path(path: impl Into<PathBuf>) -> HostKeyStore {
        HostKeyStore { path: path.into() }
    }

    /// 信任记录文件路径。
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 校验远端出示的主机密钥。
    ///
    /// - 无匹配记录：写入信任记录并返回 [`Status::FirstTime`]。
    /// - 有匹配记录且公钥一致：返回 [`Status::Trusted`]。
    /// - 有匹配记录但公钥不一致：返回 [`HostKeyError::Changed`]。
    pub fn verify(&self, host: &str, port: u16, key: &PublicKey) -> Result<Status, HostKeyError> {
        let presented = fingerprint(key);
        let entries = known_host_keys_path(host, port, &self.path).map_err(|e| match e {
            russh::keys::Error::IO(e) => HostKeyError::Io(e),
            other => HostKeyError::Parse(other.to_string()),
        })?;

        if entries.is_empty() {
            learn_known_hosts_path(host, port, key, &self.path).map_err(|e| match e {
                russh::keys::Error::IO(e) => HostKeyError::Io(e),
                other => HostKeyError::Parse(other.to_string()),
            })?;
            return Ok(Status::FirstTime {
                fingerprint: presented,
            });
        }

        // 只比较密钥本体，注释（comment）不参与比较
        let matched = entries.iter().any(|(_, recorded)| {
            recorded.algorithm() == key.algorithm() && recorded.key_data() == key.key_data()
        });

        if matched {
            Ok(Status::Trusted)
        } else {
            Err(HostKeyError::Changed {
                recorded: entries.iter().map(|(_, k)| fingerprint(k)).collect(),
                presented,
            })
        }
    }
}

/// 计算主机密钥指纹（SHA256）。
pub fn fingerprint(key: &PublicKey) -> String {
    key.fingerprint(HashAlg::Sha256).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use russh::keys::ssh_key::public::{Ed25519PublicKey, KeyData};

    /// 构造一个固定内容的测试用主机公钥。
    fn test_key(seed: u8) -> PublicKey {
        PublicKey::new(
            KeyData::Ed25519(Ed25519PublicKey([seed; 32])),
            "r_connector-test",
        )
    }

    /// 极简临时目录，避免额外依赖。
    mod tempdir {
        use std::path::{Path, PathBuf};
        use std::sync::atomic::{AtomicUsize, Ordering};

        static COUNTER: AtomicUsize = AtomicUsize::new(0);

        pub struct TempDir(PathBuf);

        impl TempDir {
            pub fn new() -> TempDir {
                let n = COUNTER.fetch_add(1, Ordering::SeqCst);
                let path = std::env::temp_dir().join(format!(
                    "r_connector_hostkey_{}_{}",
                    std::process::id(),
                    n
                ));
                std::fs::create_dir_all(&path).unwrap();
                TempDir(path)
            }

            pub fn path(&self) -> &Path {
                &self.0
            }
        }

        impl Drop for TempDir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }

    #[test]
    fn first_time_is_recorded_then_trusted() {
        let dir = tempdir::TempDir::new();
        let key = test_key(1);
        let store = HostKeyStore::with_path(dir.path().join("known_hosts"));

        let status = store.verify("example.com", 22, &key).unwrap();
        assert!(matches!(status, Status::FirstTime { .. }));
        assert!(dir.path().join("known_hosts").exists());

        let status = store.verify("example.com", 22, &key).unwrap();
        assert_eq!(status, Status::Trusted);
    }

    #[test]
    fn changed_key_is_rejected() {
        let dir = tempdir::TempDir::new();
        let key = test_key(1);
        let store = HostKeyStore::with_path(dir.path().join("known_hosts"));
        store.verify("example.com", 22, &key).unwrap();

        let err = store.verify("example.com", 22, &test_key(2)).unwrap_err();
        assert!(matches!(err, HostKeyError::Changed { .. }));
    }

    #[test]
    fn non_default_port_is_tracked_separately() {
        let dir = tempdir::TempDir::new();
        let key = test_key(1);
        let store = HostKeyStore::with_path(dir.path().join("known_hosts"));
        store.verify("example.com", 2222, &key).unwrap();

        // 22 端口视为另一台主机，不应命中已记录的 2222 端口条目
        let status = store.verify("example.com", 22, &key).unwrap();
        assert!(matches!(status, Status::FirstTime { .. }));
    }
}
