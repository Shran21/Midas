use std::{
    io,
    path::{Path, PathBuf},
};

use midas_protocol::enrollment::Kind;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// State files live in a directory only the agent (and administrators) can read.
#[derive(Clone, Debug)]
pub struct Paths {
    pub root: PathBuf,
    config: PathBuf,
}

impl Paths {
    /// Configuration next to the state, as on Windows.
    pub fn new(root: PathBuf) -> Self {
        let config = root.join("config.json");
        Self { root, config }
    }
    /// Separate configuration file, as with `/etc` on Linux.
    pub fn with_config(root: PathBuf, config: PathBuf) -> Self {
        Self { root, config }
    }
    pub fn config(&self) -> PathBuf {
        self.config.clone()
    }
    pub fn key(&self) -> PathBuf {
        self.root.join("agent.key")
    }
    pub fn certificate(&self) -> PathBuf {
        self.root.join("agent.crt")
    }
    pub fn pending(&self) -> PathBuf {
        self.root.join("pending.json")
    }
    pub fn directory(&self) -> PathBuf {
        self.root.join("directory.json")
    }
    pub fn logs(&self) -> PathBuf {
        self.root.join("logs")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Server host name; it must match the server certificate.
    pub server: String,
    /// Operational mTLS port.
    pub port: u16,
    /// Enrollment HTTPS port.
    pub enrollment_port: u16,
    /// Trust anchors for the server certificate (PEM).
    pub server_ca: String,
    pub kind: Kind,
    pub name: String,
}

impl Config {
    pub fn validate(&self) -> Result<(), &'static str> {
        let host = self.server.trim();
        if host.is_empty() || host.len() > 253 || host.contains(['/', ':', ' ']) {
            return Err("server must be a host name or IP address");
        }
        if self.port == 0 || self.enrollment_port == 0 {
            return Err("ports must not be zero");
        }
        if !self.server_ca.contains("-----BEGIN CERTIFICATE-----") {
            return Err("server_ca must contain a PEM certificate");
        }
        let name = self.name.trim();
        if name.is_empty() || name.chars().count() > 200 || name.chars().any(char::is_control) {
            return Err("name must be 1-200 printable characters");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pending {
    pub request_id: Uuid,
    pub secret: String,
}

/// Read-only directory account for the connector; only on the designated agent.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectoryAccount {
    pub user: String,
    pub password: String,
}

pub fn read_json<T: for<'a> Deserialize<'a>>(path: &Path) -> io::Result<Option<T>> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// Writes through a temporary file so a crash never leaves a half-written state file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, bytes)?;
    std::fs::rename(&temporary, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> Config {
        Config {
            server: "midas.example".into(),
            port: 8444,
            enrollment_port: 443,
            server_ca: "-----BEGIN CERTIFICATE-----\nAA==\n-----END CERTIFICATE-----\n".into(),
            kind: Kind::Workstation,
            name: "PC-01".into(),
        }
    }

    #[test]
    fn configuration_is_validated() {
        assert_eq!(config().validate(), Ok(()));
        for broken in [
            Config {
                server: "https://midas.example".into(),
                ..config()
            },
            Config {
                port: 0,
                ..config()
            },
            Config {
                server_ca: "trust me".into(),
                ..config()
            },
            Config {
                name: "\u{7}".into(),
                ..config()
            },
        ] {
            assert!(broken.validate().is_err(), "{broken:?}");
        }
        let text = r#"{"server":"m.example","port":1,"enrollment_port":2,"server_ca":"x","kind":"server","name":"n","extra":1}"#;
        assert!(serde_json::from_str::<Config>(text).is_err());
    }

    #[test]
    fn state_files_round_trip_atomically() {
        let root = std::env::temp_dir().join(format!("midas-agent-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let paths = Paths::new(root.clone());
        assert_eq!(read_json::<Pending>(&paths.pending()).unwrap(), None);
        let pending = Pending {
            request_id: Uuid::new_v4(),
            secret: "s".repeat(64),
        };
        write_atomic(&paths.pending(), &serde_json::to_vec(&pending).unwrap()).unwrap();
        assert_eq!(
            read_json::<Pending>(&paths.pending()).unwrap(),
            Some(pending)
        );
        assert!(!paths.pending().with_extension("tmp").exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
