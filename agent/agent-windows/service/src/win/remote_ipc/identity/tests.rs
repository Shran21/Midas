use super::*;
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
};

pub(crate) fn process(pid: u32) -> OwnedHandle {
    // SAFETY: test-only lookup of a known spawned process or this test process; handle is owned below.
    let handle = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            0,
            pid,
        )
    };
    assert!(!handle.is_null(), "{}", io::Error::last_os_error());
    // SAFETY: OpenProcess returned one owned, non-null handle.
    unsafe { OwnedHandle::from_raw_handle(handle) }
}

pub(crate) fn peer(process: OwnedHandle) -> Peer {
    let snapshot = Snapshot::read(&process).unwrap();
    Peer {
        role: Role::Worker {
            kind: Worker::User {
                user_sid: snapshot.token.user.clone(),
                logon_id: snapshot.token.logon,
            },
            session: snapshot.token.session,
        },
        process,
        snapshot,
    }
}

pub(crate) fn local() -> Peer {
    peer(process(std::process::id()))
}

fn normal() -> Snapshot {
    Snapshot {
        pid: 12,
        image: PathBuf::from(r"C:\Program Files\MIDAS\worker.exe"),
        token: Token {
            id: 10,
            modified: 20,
            logon: 30,
            session: 4,
            user: "S-1-5-21-1-2-3-1001".into(),
            logon_sid: Some("S-1-5-5-0-30".into()),
            integrity: "S-1-16-8192".into(),
            elevated: false,
            ui_access: false,
            app_container: false,
        },
    }
}

#[test]
fn worker_roles_require_the_exact_session_logon_and_privilege() {
    let mut sample = normal();
    let user = Role::Worker {
        kind: Worker::User {
            user_sid: sample.token.user.clone(),
            logon_id: 30,
        },
        session: 4,
    };
    let privileged = Role::Worker {
        kind: Worker::Privileged,
        session: 4,
    };
    assert!(sample.allows(&user));
    assert!(!sample.allows(&privileged));
    assert!(!sample.allows(&Role::Broker));
    for token in [
        Token {
            elevated: true,
            ..sample.token.clone()
        },
        Token {
            ui_access: true,
            ..sample.token.clone()
        },
        Token {
            app_container: true,
            ..sample.token.clone()
        },
        Token {
            session: 5,
            ..sample.token.clone()
        },
        Token {
            session: 0,
            ..sample.token.clone()
        },
        Token {
            session: u32::MAX,
            ..sample.token.clone()
        },
        Token {
            logon: 31,
            ..sample.token.clone()
        },
        Token {
            user: "S-1-5-21-1-2-3-1002".into(),
            ..sample.token.clone()
        },
        Token {
            integrity: "S-1-16-12288".into(),
            ..sample.token.clone()
        },
        Token {
            logon_sid: None,
            ..sample.token.clone()
        },
    ] {
        assert!(
            !Snapshot {
                token,
                ..sample.clone()
            }
            .allows(&user)
        );
    }
    sample.token.user = SYSTEM.into();
    sample.token.integrity = "S-1-16-16384".into();
    sample.token.elevated = true;
    assert!(sample.allows(&privileged));
    assert!(!sample.allows(&user));
    assert!(!sample.allows(&Role::Broker));
    sample.token.session = 0;
    assert!(sample.allows(&Role::Broker));
    assert!(!sample.allows(&privileged));
}

#[test]
fn pinned_identity_rejects_changes_and_wrong_images() {
    let mut pinned = local();
    let pid = std::process::id();
    pinned.check(pid).unwrap();
    assert!(pinned.check(pid.wrapping_add(1)).is_err());
    pinned.snapshot.token.modified ^= 1;
    assert!(pinned.check(pid).is_err());
    pinned.snapshot.token.modified ^= 1;
    pinned.snapshot.token.id ^= 1;
    assert!(pinned.check(pid).is_err());
    assert!(Peer::broker(process(pid), Path::new(r"C:\wrong.exe")).is_err());
    assert!(
        Peer::worker(
            process(pid),
            &std::env::current_exe().unwrap(),
            Worker::Privileged,
            u32::MAX
        )
        .is_err()
    );
}

#[test]
fn acl_grants_the_logon_only_data_access_and_never_an_ordinary_sid_to_system_workers() {
    let mut pinned = local();
    pinned.snapshot = normal();
    let acl = pinned.acl().unwrap();
    assert_eq!(
        acl,
        "D:P(D;;GA;;;NU)(A;;GA;;;SY)(A;;0x0012008b;;;S-1-5-5-0-30)"
    );
    pinned.role = Role::Worker {
        kind: Worker::Privileged,
        session: 4,
    };
    assert_eq!(
        pinned.acl().unwrap(),
        "D:P(D;;GA;;;NU)(A;;GA;;;SY)(A;;0x0012008b;;;S-1-5-18)"
    );
    pinned.role = Role::Broker;
    assert!(pinned.acl().is_err());
}
