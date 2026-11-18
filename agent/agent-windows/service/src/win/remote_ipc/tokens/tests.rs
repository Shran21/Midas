use windows_sys::Win32::{
    Foundation::{ERROR_ACCESS_DENIED, GetHandleInformation, HANDLE_FLAG_INHERIT},
    Security::{AdjustTokenPrivileges, ImpersonateSelf, RevertToSelf},
};

use super::super::identity::tests::{local, sample_token};
use super::*;

fn target() -> OsSession {
    OsSession {
        id: 4,
        generation: uuid::Uuid::new_v4().as_bytes().to_vec(),
        presence: Presence::Occupied as i32,
        station: "Console".into(),
        account: "EXAMPLE\\user".into(),
        console: true,
        locked: Some(false),
    }
}

#[test]
fn selected_generation_account_and_presence_must_still_match() {
    let selected = target();
    for user in [true, false] {
        check_target(&selected, Some(vec![selected.clone()]), user).unwrap();
        for changed in [
            OsSession {
                id: 5,
                ..selected.clone()
            },
            OsSession {
                generation: uuid::Uuid::new_v4().as_bytes().to_vec(),
                ..selected.clone()
            },
            OsSession {
                account: "EXAMPLE\\other".into(),
                ..selected.clone()
            },
            OsSession {
                station: "RDP-Tcp#1".into(),
                ..selected.clone()
            },
            OsSession {
                console: false,
                ..selected.clone()
            },
            OsSession {
                locked: Some(true),
                ..selected.clone()
            },
            OsSession {
                presence: Presence::Unknown as i32,
                ..selected.clone()
            },
        ] {
            assert!(check_target(&selected, Some(vec![changed]), user).is_err());
        }
        assert!(check_target(&selected, None, user).is_err());
        assert!(check_target(&selected, Some(vec![]), user).is_err());
        assert!(
            check_target(
                &selected,
                Some(vec![selected.clone(), selected.clone()]),
                user
            )
            .is_err()
        );
    }
}

#[test]
fn vacant_unknown_or_malformed_sessions_cannot_supply_a_user_worker() {
    let selected = target();
    let empty = OsSession {
        presence: Presence::Vacant as i32,
        account: String::new(),
        ..selected.clone()
    };
    check_target(&empty, Some(vec![empty.clone()]), false).unwrap();
    assert!(check_target(&empty, Some(vec![empty.clone()]), true).is_err());
    for invalid in [
        OsSession {
            id: 0,
            ..selected.clone()
        },
        OsSession {
            id: u32::MAX,
            ..selected.clone()
        },
        OsSession {
            generation: vec![0; 16],
            ..selected.clone()
        },
        OsSession {
            presence: Presence::Unknown as i32,
            ..selected.clone()
        },
        OsSession {
            account: String::new(),
            ..selected.clone()
        },
        OsSession {
            locked: None,
            ..selected.clone()
        },
        OsSession {
            presence: Presence::Vacant as i32,
            ..selected
        },
    ] {
        assert!(check_target(&invalid, Some(vec![invalid.clone()]), false).is_err());
        assert!(check_target(&invalid, Some(vec![invalid.clone()]), true).is_err());
    }
}

#[test]
fn both_sides_of_the_linked_logon_pair_are_pinned() {
    let binding = UserBinding {
        source: sample_token(20, 1),
        selected: sample_token(30, 3),
    };
    assert!(binding.matches(&UserBinding {
        source: sample_token(20, 100),
        selected: sample_token(30, 200)
    }));
    assert!(!binding.matches(&UserBinding {
        source: sample_token(21, 1),
        selected: sample_token(30, 3)
    }));
    assert!(!binding.matches(&UserBinding {
        source: sample_token(20, 1),
        selected: sample_token(31, 3)
    }));
}

#[test]
fn only_verified_service_brokers_can_prepare_tokens() {
    let broker = local();
    let observer = Arc::new(Observer::default());
    assert!(PreparedToken::user(&broker, observer.clone(), target()).is_err());
    assert!(PreparedToken::privileged(&broker, observer, target()).is_err());
}

#[test]
fn native_token_duplication_preserves_source_and_limits_handle_rights() {
    let original = current_token(TOKEN_QUERY | TOKEN_DUPLICATE).unwrap();
    let before = Token::read(&original).unwrap();
    let duplicate = duplicate(&original, LAUNCH_ACCESS).unwrap();
    let copied = Token::read(&duplicate).unwrap();
    assert!(before.same_logon(&copied));
    assert_ne!(before, copied);
    let reduced = reduce(&duplicate).unwrap();
    assert_eq!(copied, Token::read(&reduced).unwrap());
    for token in [&duplicate, &reduced] {
        let mut flags = 0;
        // SAFETY: queries a live owned token handle into a valid DWORD output.
        check(unsafe { GetHandleInformation(token.as_raw_handle(), &mut flags) }).unwrap();
        assert_eq!(flags & HANDLE_FLAG_INHERIT, 0);
        // SAFETY: disable-all needs no privilege-array pointers; only an unassigned test copy is used.
        let adjusted = unsafe {
            AdjustTokenPrivileges(
                token.as_raw_handle(),
                1,
                ptr::null(),
                0,
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };
        assert_eq!(adjusted, 0);
        assert_eq!(
            io::Error::last_os_error().raw_os_error(),
            Some(ERROR_ACCESS_DENIED as i32)
        );
    }
    assert_eq!(before, Token::read(&original).unwrap());
}

#[test]
fn only_an_explicit_missing_user_token_confirms_vacancy() {
    vacant(Err(io::Error::from_raw_os_error(ERROR_NO_TOKEN as i32))).unwrap();
    for error in [ERROR_ACCESS_DENIED as i32, 1314, 87] {
        assert!(vacant(Err(io::Error::from_raw_os_error(error))).is_err());
    }
    assert!(vacant(current_token(TOKEN_QUERY)).is_err());
}

struct Revert;
impl Drop for Revert {
    fn drop(&mut self) {
        // SAFETY: removes only this dedicated test thread's own temporary impersonation token.
        unsafe {
            RevertToSelf();
        }
    }
}

#[test]
fn thread_impersonation_is_refused_without_changing_the_process_token() {
    std::thread::spawn(|| {
        no_impersonation().unwrap();
        let process = current_token(TOKEN_QUERY).unwrap();
        let before = Token::read(&process).unwrap();
        // SAFETY: impersonates only the test account on this dedicated test thread.
        check(unsafe { ImpersonateSelf(SecurityIdentification) }).unwrap();
        let guard = Revert;
        assert!(no_impersonation().is_err());
        drop(guard);
        no_impersonation().unwrap();
        assert_eq!(before, Token::read(&process).unwrap());
    })
    .join()
    .unwrap();
}
