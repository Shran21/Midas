use windows_sys::Win32::{
    Foundation::{ERROR_ACCESS_DENIED, ERROR_NOT_ALL_ASSIGNED},
    Security::{
        DuplicateTokenEx, SE_CHANGE_NOTIFY_NAME, SecurityIdentification, TOKEN_DUPLICATE,
        TokenPrimary,
    },
};

use super::*;

fn duplicate() -> OwnedHandle {
    let original = current_token(TOKEN_QUERY | TOKEN_DUPLICATE).unwrap();
    let mut copy = ptr::null_mut();
    assert_ne!(
        // SAFETY: makes an unassigned test copy; no process or thread receives this token.
        unsafe {
            DuplicateTokenEx(
                original.as_raw_handle(),
                TOKEN_QUERY | TOKEN_ADJUST_PRIVILEGES,
                ptr::null(),
                SecurityIdentification,
                TokenPrimary,
                &mut copy,
            )
        },
        0
    );
    // SAFETY: the successful native call returned one owned token handle.
    unsafe { OwnedHandle::from_raw_handle(copy) }
}

fn disable(token: &OwnedHandle) {
    assert_ne!(
        // SAFETY: disables privileges only on this unassigned test token, without an input/output array.
        unsafe {
            AdjustTokenPrivileges(
                token.as_raw_handle(),
                1,
                ptr::null(),
                0,
                ptr::null_mut(),
                ptr::null_mut(),
            )
        },
        0
    );
}

#[test]
fn normal_process_cannot_initialize_a_system_broker_or_change_its_privileges() {
    let token = current_token(TOKEN_QUERY).unwrap();
    let before = Token::read(&token).unwrap();
    assert!(
        !before.allows(&Role::Broker),
        "run this unit test as the normal test account"
    );
    let previous = privileges(&token).unwrap();
    assert!(Broker::initialize(&std::env::current_exe().unwrap()).is_err());
    assert_eq!(before, Token::read(&token).unwrap());
    assert_eq!(previous, privileges(&token).unwrap());
}

#[test]
fn successful_adjustment_restores_only_its_changes_on_finish_and_drop() {
    let source = current_token(TOKEN_QUERY).unwrap();
    let source_before = Token::read(&source).unwrap();
    let id = lookup(SE_CHANGE_NOTIFY_NAME).unwrap();
    for explicit in [false, true] {
        let token = duplicate();
        disable(&token);
        let observed = token.try_clone().unwrap();
        let before = Token::read(&observed).unwrap();
        let previous = privileges(&observed).unwrap();
        assert!(!source_before.same_primary_identity(&before));
        let mut guard = Adjustment::enable(token, &[id]).unwrap();
        enabled(&observed, &[id]).unwrap();
        let adjusted = Token::read(&observed).unwrap();
        assert_ne!(before, adjusted);
        assert!(before.same_primary_identity(&adjusted));
        if explicit {
            guard.restore().unwrap();
            let restored = Token::read(&observed).unwrap();
            guard.restore().unwrap();
            assert_eq!(restored, Token::read(&observed).unwrap());
        }
        drop(guard);
        assert_eq!(previous, privileges(&observed).unwrap());
    }
    assert_eq!(source_before, Token::read(&source).unwrap());
}

#[test]
fn partial_native_success_is_an_error_and_rolls_back_every_applied_change() {
    let token = duplicate();
    disable(&token);
    let observed = token.try_clone().unwrap();
    let previous = privileges(&observed).unwrap();
    let valid = lookup(SE_CHANGE_NOTIFY_NAME).unwrap();
    let missing = LUID {
        LowPart: u32::MAX,
        HighPart: i32::MAX,
    };
    let requested = Privileges {
        count: 2,
        entries: [
            LUID_AND_ATTRIBUTES {
                Luid: valid,
                Attributes: SE_PRIVILEGE_ENABLED,
            },
            LUID_AND_ATTRIBUTES {
                Luid: missing,
                Attributes: SE_PRIVILEGE_ENABLED,
            },
            LUID_AND_ATTRIBUTES::default(),
        ],
    };
    // SAFETY: reproduces Windows partial success on an unassigned test token with two bounded entries.
    let (ok, error) = unsafe {
        let ok = AdjustTokenPrivileges(
            token.as_raw_handle(),
            0,
            ptr::from_ref(&requested).cast(),
            0,
            ptr::null_mut(),
            ptr::null_mut(),
        );
        (ok, GetLastError())
    };
    assert_ne!(ok, 0);
    assert_eq!(error, ERROR_NOT_ALL_ASSIGNED);
    enabled(&observed, &[valid]).unwrap();
    disable(&token);
    let error = Adjustment::enable(token, &[valid, missing]).err().unwrap();
    assert_eq!(error.raw_os_error(), Some(ERROR_NOT_ALL_ASSIGNED as i32));
    assert_eq!(previous, privileges(&observed).unwrap());
}

#[test]
fn already_enabled_privileges_are_not_disabled_by_cleanup() {
    let token = duplicate();
    let id = lookup(SE_CHANGE_NOTIFY_NAME).unwrap();
    enabled(&token, &[id]).unwrap();
    let observed = token.try_clone().unwrap();
    let previous = privileges(&observed).unwrap();
    let before = Token::read(&observed).unwrap();
    let guard = Adjustment::enable(token, &[id]).unwrap();
    assert_eq!(guard.previous.count, 0);
    drop(guard);
    assert_eq!(previous, privileges(&observed).unwrap());
    assert_eq!(before, Token::read(&observed).unwrap());
}

#[test]
fn invalid_requests_and_missing_handle_rights_change_nothing() {
    let id = lookup(SE_CHANGE_NOTIFY_NAME).unwrap();
    for ids in [vec![], vec![id, id], vec![id; 4]] {
        let token = duplicate();
        let observed = token.try_clone().unwrap();
        let before = Token::read(&observed).unwrap();
        assert!(Adjustment::enable(token, &ids).is_err());
        assert_eq!(before, Token::read(&observed).unwrap());
    }
    let token = current_token(TOKEN_QUERY).unwrap();
    let observed = token.try_clone().unwrap();
    let before = Token::read(&observed).unwrap();
    let error = Adjustment::enable(token, &[id]).err().unwrap();
    assert_eq!(error.raw_os_error(), Some(ERROR_ACCESS_DENIED as i32));
    assert_eq!(before, Token::read(&observed).unwrap());
}

#[test]
fn only_one_startup_owner_can_hold_the_process_privileges() {
    static TEST_ACTIVE: AtomicBool = AtomicBool::new(false);
    let owner = Exclusive::acquire(&TEST_ACTIVE).unwrap();
    assert_eq!(
        Exclusive::acquire(&TEST_ACTIVE).err().unwrap().kind(),
        io::ErrorKind::AlreadyExists
    );
    drop(owner);
    drop(Exclusive::acquire(&TEST_ACTIVE).unwrap());
}
