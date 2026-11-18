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
            elevation_type: TokenElevationTypeLimited as u32,
            ui_access: false,
            app_container: false,
        },
    }
}

pub(in crate::win::remote_ipc) fn sample_token(logon: u64, object: u64) -> Token {
    let mut token = normal().token;
    token.logon = logon;
    token.logon_sid = Some(format!("S-1-5-5-0-{logon}"));
    token.id = object;
    token.modified = object + 1;
    token
}

#[test]
fn linked_selection_keeps_user_and_session_without_conflating_logon_ids() {
    let limited = normal().token;
    assert!(!limited.needs_linked(4).unwrap());
    let full = Token {
        elevated: true,
        elevation_type: TokenElevationTypeFull as u32,
        integrity: "S-1-16-12288".into(),
        logon: 90,
        ..limited.clone()
    };
    assert!(full.needs_linked(4).unwrap());
    assert_eq!(
        full.linked_worker(&limited, 4).unwrap(),
        limited.user_worker(4).unwrap()
    );
    for invalid in [
        Token {
            user: "S-1-5-21-1-2-3-1002".into(),
            ..limited.clone()
        },
        Token {
            session: 5,
            ..limited.clone()
        },
        Token {
            elevated: true,
            ..limited.clone()
        },
        Token {
            elevation_type: TokenElevationTypeFull as u32,
            ..limited.clone()
        },
        Token {
            elevation_type: TokenElevationTypeDefault as u32,
            ..limited.clone()
        },
        Token {
            integrity: "S-1-16-12288".into(),
            ..limited.clone()
        },
        Token {
            ui_access: true,
            ..limited.clone()
        },
        Token {
            app_container: true,
            ..limited.clone()
        },
        Token {
            logon_sid: None,
            ..limited.clone()
        },
    ] {
        assert!(full.linked_worker(&invalid, 4).is_err());
    }
    let unfiltered = Token {
        elevation_type: TokenElevationTypeDefault as u32,
        ..full
    };
    assert!(unfiltered.needs_linked(4).is_err());
    assert!(limited.linked_worker(&limited, 4).is_err());
}

#[test]
fn fresh_token_objects_preserve_a_logon_but_changed_identity_does_not() {
    let token = normal().token;
    assert!(token.same_logon(&Token {
        id: 99,
        modified: 100,
        ..token.clone()
    }));
    for changed in [
        Token {
            logon: 31,
            ..token.clone()
        },
        Token {
            logon_sid: Some("S-1-5-5-0-31".into()),
            ..token.clone()
        },
        Token {
            session: 5,
            ..token.clone()
        },
        Token {
            user: "S-1-5-21-1-2-3-1002".into(),
            ..token.clone()
        },
        Token {
            elevated: true,
            ..token.clone()
        },
        Token {
            elevation_type: TokenElevationTypeFull as u32,
            ..token.clone()
        },
        Token {
            ui_access: true,
            ..token.clone()
        },
        Token {
            app_container: true,
            ..token.clone()
        },
        Token {
            integrity: "S-1-16-12288".into(),
            ..token.clone()
        },
    ] {
        assert!(!token.same_logon(&changed));
    }
    for session in [0, 5, u32::MAX] {
        assert!(token.observed_user(session).is_err());
    }
    assert!(
        Token {
            user: SYSTEM.into(),
            ..token
        }
        .observed_user(4)
        .is_err()
    );
}

#[test]
fn duplicating_a_peer_preserves_the_original_identity_pin() {
    let mut pinned = local();
    let copied = pinned.duplicate().unwrap();
    assert_eq!(pinned.snapshot, copied.snapshot);
    copied.check(std::process::id()).unwrap();
    pinned.snapshot.token.modified ^= 1;
    assert!(pinned.duplicate().is_err());
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
