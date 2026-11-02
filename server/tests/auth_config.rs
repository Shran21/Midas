use clap::Parser;
use midas_server::config::{Arguments, authentication_key};

#[test]
fn browser_authentication_requires_complete_tls_configuration() {
    for args in [
        vec!["server", "--auth-origin", "https://midas.example"],
        vec!["server", "--auth-key-file", "key"],
    ] {
        assert!(Arguments::try_parse_from(args).is_err());
    }
    let insecure = Arguments::try_parse_from([
        "server",
        "--auth-origin",
        "https://midas.example",
        "--auth-key-file",
        "key",
    ])
    .unwrap();
    assert!(insecure.validate().is_err());
    let secure = Arguments::try_parse_from([
        "server",
        "--tls-cert",
        "cert",
        "--tls-key",
        "tls-key",
        "--auth-origin",
        "https://midas.example",
        "--auth-key-file",
        "key",
    ])
    .unwrap();
    secure.validate().unwrap();
    assert_eq!(secure.session_seconds, 43_200);
    assert_eq!(secure.session_idle_seconds, 1_800);
}

#[test]
fn key_file_is_fixed_size_private_and_not_regenerated() {
    let path = std::env::temp_dir().join(format!("midas-key-test-{}", uuid::Uuid::new_v4()));
    assert!(authentication_key(&path).is_err());
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(&path).unwrap();
    drop(file);
    for length in [0, 31, 33, 1_000] {
        std::fs::write(&path, vec![1; length]).unwrap();
        assert!(authentication_key(&path).is_err());
    }
    std::fs::write(&path, [0; 32]).unwrap();
    assert!(authentication_key(&path).is_err());
    let mut bytes = [0; 32];
    getrandom::fill(&mut bytes).unwrap();
    std::fs::write(&path, bytes).unwrap();
    assert_eq!(*authentication_key(&path).unwrap(), bytes);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(authentication_key(&path).is_err());
    }
    std::fs::remove_file(path).unwrap();
}
