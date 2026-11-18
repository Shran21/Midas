use std::time::Duration;
use windows_sys::Win32::{
    Foundation::{ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, WAIT_OBJECT_0},
    Security::{
        ImpersonateLoggedOnUser, LOGON32_LOGON_INTERACTIVE, LOGON32_PROVIDER_DEFAULT, LogonUserW,
        RevertToSelf, SetTokenInformation, TOKEN_ADJUST_DEFAULT, TOKEN_ADJUST_GROUPS,
        TOKEN_ADJUST_PRIVILEGES, TOKEN_ADJUST_SESSIONID, TOKEN_IMPERSONATE, TOKEN_QUERY_SOURCE,
        TokenSessionId,
    },
    System::{
        JobObjects::IsProcessInJob,
        RemoteDesktop::WTSGetActiveConsoleSessionId,
        Threading::{
            CreateEventW, EVENT_MODIFY_STATE, GetCurrentProcess, OpenEventW, OpenProcess,
            OpenProcessToken, PROCESS_CREATE_PROCESS, PROCESS_DUP_HANDLE,
            PROCESS_QUERY_INFORMATION, PROCESS_TERMINATE, PROCESS_VM_OPERATION, PROCESS_VM_READ,
            PROCESS_VM_WRITE, SetEvent, WaitForSingleObject,
        },
    },
};
use zeroize::Zeroizing;

use super::super::super::sessions::tests::Fixture;
use super::super::{identity::tests::local, startup::Broker};
use super::*;

const CHILD: &str = "win::remote_ipc::broker::tests::companion_child";

#[test]
fn ordinary_process_cannot_prepare_a_system_companion() {
    let token = current_token(TOKEN_QUERY).unwrap();
    let before = Token::read(&token).unwrap();
    let fixture = Fixture::new();
    assert!(Preparation::new(&local(), local(), fixture.binding()).is_err());
    assert_eq!(before, Token::read(&token).unwrap());
}

#[test]
fn duplication_creates_a_separate_noninherited_token_without_changing_its_source() {
    let source = current_token(TOKEN_QUERY | TOKEN_DUPLICATE).unwrap();
    let before = Token::read(&source).unwrap();
    let mut attributes = access("S-1-5-5-123-456", TOKEN_QUERY, TOKEN_ALL_ACCESS).unwrap();
    let copy = copy(&source, &mut attributes).unwrap();
    let after = Token::read(&copy).unwrap();
    assert!(before.same_logon(&after));
    assert!(!before.same_primary_identity(&after));
    assert_eq!(before, Token::read(&source).unwrap());
    let mut flags = 0;
    assert_ne!(
        // SAFETY: copy is owned and flags is a writable output.
        unsafe {
            windows_sys::Win32::Foundation::GetHandleInformation(copy.as_raw_handle(), &mut flags)
        },
        0
    );
    assert_eq!(
        flags & windows_sys::Win32::Foundation::HANDLE_FLAG_INHERIT,
        0
    );
}

#[test]
fn token_acl_update_is_scoped_read_back_and_requires_write_dac() {
    use super::super::identity::privileges;
    use windows_sys::Win32::Foundation::DuplicateHandle;

    let source = current_token(TOKEN_QUERY | TOKEN_DUPLICATE | 0x0002_0000).unwrap();
    let original = Token::read(&source).unwrap();
    let original_acl = read_dacl(&source).unwrap();
    let original_privileges = privileges(&source).unwrap();
    let mut raw = ptr::null_mut();
    assert_ne!(
        // SAFETY: creates an unassigned token copy; the current process token is not changed.
        unsafe {
            DuplicateTokenEx(
                source.as_raw_handle(),
                TOKEN_QUERY | 0x0006_0000,
                ptr::null(),
                SecurityIdentification,
                TokenPrimary,
                &mut raw,
            )
        },
        0
    );
    // SAFETY: successful duplication returned this owned token copy.
    let token = unsafe { OwnedHandle::from_raw_handle(raw) };
    let before = Token::read(&token).unwrap();
    assert!(!before.same_primary_identity(&original));
    let attributes = access("S-1-5-5-123-456", TOKEN_QUERY, TOKEN_ALL_ACCESS).unwrap();
    replace_dacl(&token, &attributes).unwrap();
    let configured = read_dacl(&token).unwrap();
    assert!(configured.1, "DACL must remain protected");
    assert_ne!(configured, original_acl);
    assert_eq!(before, Token::read(&token).unwrap());
    assert_eq!(privileges(&token).unwrap(), original_privileges);

    let mut raw = ptr::null_mut();
    assert_ne!(
        // SAFETY: reduces an existing handle's rights without changing either token object.
        unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                token.as_raw_handle(),
                GetCurrentProcess(),
                &mut raw,
                TOKEN_QUERY,
                0,
                0,
            )
        },
        0
    );
    // SAFETY: successful duplication returned one owned query-only handle.
    let query = unsafe { OwnedHandle::from_raw_handle(raw) };
    let forbidden = access("S-1-5-5-123-789", TOKEN_ALL_ACCESS, TOKEN_ALL_ACCESS).unwrap();
    assert_eq!(
        replace_dacl(&query, &forbidden).unwrap_err().raw_os_error(),
        Some(ERROR_ACCESS_DENIED as i32)
    );
    assert_eq!(read_dacl(&token).unwrap(), configured);
    assert_eq!(Token::read(&source).unwrap(), original);
    assert_eq!(read_dacl(&source).unwrap(), original_acl);
    assert_eq!(privileges(&source).unwrap(), original_privileges);
}

#[test]
#[ignore = "child fixture, launched only by its owning test"]
fn companion_child() {
    let name = event_name(std::process::id());
    // SAFETY: opens only the fixture event created before this child was resumed.
    let event = unsafe { OpenEventW(EVENT_MODIFY_STATE, 0, name.as_ptr()) };
    assert!(!event.is_null(), "ready event missing");
    // SAFETY: the successful native call returned one owned event handle.
    let event = unsafe { OwnedHandle::from_raw_handle(event) };
    // SAFETY: the event remains owned through this signaling call.
    assert_ne!(unsafe { SetEvent(event.as_raw_handle()) }, 0);
    std::thread::sleep(Duration::from_secs(60));
}

fn event_name(pid: u32) -> Vec<u16> {
    super::super::super::wide(&format!("Global\\MIDAS-BrokerTrialReady-{pid}"))
}

fn ready_event(pid: u32) -> OwnedHandle {
    let name = event_name(pid);
    // SAFETY: a non-inherited manual-reset event with this SYSTEM fixture's default descriptor.
    let raw = unsafe { CreateEventW(ptr::null(), 1, 0, name.as_ptr()) };
    assert!(!raw.is_null());
    // SAFETY: inspect the preceding event creation's last-error value immediately.
    let existed = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    // SAFETY: successful CreateEventW returns one owned event handle.
    let event = unsafe { OwnedHandle::from_raw_handle(raw) };
    assert!(!existed, "ready event already exists");
    event
}

struct Impersonation;

impl Impersonation {
    fn enter(token: &OwnedHandle) -> Self {
        // SAFETY: this synchronous test owns the token and reverts on this same OS thread.
        assert_ne!(unsafe { ImpersonateLoggedOnUser(token.as_raw_handle()) }, 0);
        Self
    }
}

impl Drop for Impersonation {
    fn drop(&mut self) {
        // SAFETY: this guard restores the calling test thread, including panic cleanup.
        if unsafe { RevertToSelf() } == 0 {
            std::process::abort();
        }
    }
}

fn logon(session: u32) -> OwnedHandle {
    let name = super::super::super::wide(&std::env::var("MIDAS_SYSTEM_TRIAL_USER").unwrap());
    let password = Zeroizing::new(std::env::var("MIDAS_SYSTEM_TRIAL_PASSWORD").unwrap());
    let password = Zeroizing::new(super::super::super::wide(&password));
    let domain = super::super::super::wide(".");
    let mut raw = ptr::null_mut();
    // SAFETY: all strings are NUL-terminated and alive; only the isolated CI fixture owns this account.
    let ok = unsafe {
        LogonUserW(
            name.as_ptr(),
            domain.as_ptr(),
            password.as_ptr(),
            LOGON32_LOGON_INTERACTIVE,
            LOGON32_PROVIDER_DEFAULT,
            &mut raw,
        )
    };
    assert_ne!(ok, 0, "logon: {}", io::Error::last_os_error());
    // SAFETY: successful LogonUserW returns one owned primary token.
    let token = unsafe { OwnedHandle::from_raw_handle(raw) };
    // SAFETY: moves only this fixture's new token to the existing console session.
    let ok = unsafe {
        SetTokenInformation(
            token.as_raw_handle(),
            TokenSessionId,
            ptr::from_ref(&session).cast(),
            size_of::<u32>() as u32,
        )
    };
    assert_ne!(ok, 0, "test token session: {}", io::Error::last_os_error());
    Token::read(&token).unwrap().user_worker(session).unwrap();
    token
}

fn user_process(token: &OwnedHandle, image: &Path) -> Process {
    let application = image_path(image).unwrap();
    let mut command =
        super::super::super::wide(&format!("\"{}\" --exact {CHILD}", image.display()));
    let environment = environment().unwrap();
    let mut desktop = [0u16];
    Process::create(Some(&mut desktop), |startup, flags, information| {
        // SAFETY: this CI-only standard-user fixture stays suspended; all buffers outlive creation.
        unsafe {
            CreateProcessAsUserW(
                token.as_raw_handle(),
                application.as_ptr(),
                command.as_mut_ptr(),
                ptr::null(),
                ptr::null(),
                0,
                flags,
                environment.as_ptr().cast(),
                ptr::null(),
                startup,
                information,
            )
        }
    })
    .unwrap()
}

fn token_access(process: &OwnedHandle, rights: u32) -> io::Result<OwnedHandle> {
    let mut raw = ptr::null_mut();
    // SAFETY: the retained process handle is valid; raw is a writable output.
    if unsafe { OpenProcessToken(process.as_raw_handle(), rights, &mut raw) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful OpenProcessToken returns one owned token handle.
    Ok(unsafe { OwnedHandle::from_raw_handle(raw) })
}

fn process_access(pid: u32, rights: u32) -> io::Result<OwnedHandle> {
    // SAFETY: the test retains the child's creation handle for this entire access check.
    let raw = unsafe { OpenProcess(rights, 0, pid) };
    if raw.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful OpenProcess returns one owned process handle.
    Ok(unsafe { OwnedHandle::from_raw_handle(raw) })
}

fn refused<T>(result: io::Result<T>) {
    match result {
        Err(error) => assert_eq!(error.raw_os_error(), Some(ERROR_ACCESS_DENIED as i32)),
        Ok(_) => panic!("unexpected access to the SYSTEM companion"),
    }
}

#[test]
#[ignore = "requires the disposable Windows CI SYSTEM fixture"]
fn isolated_system_query_access() {
    assert_eq!(std::env::var("MIDAS_SYSTEM_TRIAL").as_deref(), Ok("1"));
    assert_eq!(std::env::var("GITHUB_ACTIONS").as_deref(), Ok("true"));
    let mut contained = 0;
    assert_ne!(
        // SAFETY: queries this fixture process, without changing any parent or child job.
        unsafe { IsProcessInJob(GetCurrentProcess(), ptr::null_mut(), &mut contained) },
        0
    );
    assert_eq!(
        contained, 0,
        "SYSTEM fixture must not inherit the scheduler's session-zero job"
    );
    let image = std::env::current_exe().unwrap();
    let owner = Broker::initialize(&image).unwrap();
    println!("service identity prepared");
    let original = current_token(TOKEN_QUERY).unwrap();
    let source = Token::read(&original).unwrap();
    // SAFETY: reads the existing console session without modifying it.
    let session = unsafe { WTSGetActiveConsoleSessionId() };
    assert!(
        session != 0 && session != u32::MAX,
        "CI requires an existing console session"
    );
    let user_token = logon(session);
    let other_logon = logon(session);
    println!("two standard-user logons acquired");
    let user = user_process(&user_token, &image);
    println!("suspended user fixture created");
    let role = Token::read(&user_token)
        .unwrap()
        .user_worker(session)
        .unwrap();
    let peer = Peer::worker(user.query_handle().unwrap(), &image, role, session).unwrap();
    let fixture = Fixture::new();
    let child = SuspendedBroker::create(
        owner.peer().unwrap(),
        peer,
        fixture.binding(),
        &image,
        &format!("--exact {CHILD} --ignored --nocapture"),
    )
    .unwrap();
    let process = child.process.query_handle().unwrap();
    println!("suspended SYSTEM companion created");
    let pid = child.process.pid().unwrap();
    let pinned = child.peer().unwrap();
    let current = Token::read(&token_access(&process, TOKEN_QUERY).unwrap()).unwrap();
    assert!(current.allows(&Role::Broker));
    {
        let _guard = Impersonation::enter(&user_token);
        assert_eq!(
            current,
            Token::read(
                &token_access(&process, TOKEN_QUERY).expect("query actual companion token")
            )
            .unwrap()
        );
        println!("selected logon queried the actual SYSTEM token");
        let opened =
            process_access(pid, PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE).unwrap();
        let verified =
            Peer::broker(opened, &image).expect("pin actual companion identity and session");
        verified.check(pid).unwrap();
        println!("selected logon pinned and rechecked the actual companion identity and session");
        assert_eq!(
            current,
            Token::read(&token_access(&process, TOKEN_QUERY).unwrap()).unwrap()
        );
        for rights in [
            TOKEN_DUPLICATE,
            TOKEN_ASSIGN_PRIMARY,
            TOKEN_IMPERSONATE,
            TOKEN_ADJUST_DEFAULT,
            TOKEN_ADJUST_GROUPS,
            TOKEN_ADJUST_PRIVILEGES,
            TOKEN_ADJUST_SESSIONID,
            TOKEN_QUERY_SOURCE,
            0x0004_0000,
            0x0008_0000,
            0x0001_0000,
        ] {
            refused(token_access(&process, rights));
        }
        for rights in [
            PROCESS_QUERY_INFORMATION,
            PROCESS_DUP_HANDLE,
            PROCESS_CREATE_PROCESS,
            PROCESS_TERMINATE,
            PROCESS_VM_OPERATION,
            PROCESS_VM_READ,
            PROCESS_VM_WRITE,
            0x0004_0000,
            0x0008_0000,
        ] {
            refused(process_access(pid, rights));
        }
    }
    println!("selected logon cannot modify the companion process or token");
    {
        let _guard = Impersonation::enter(&other_logon);
        refused(token_access(&process, TOKEN_QUERY));
        refused(process_access(
            pid,
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
        ));
        assert!(pinned.check(pid).is_err());
    }
    println!("the other logon cannot query the companion process or token");
    pinned.check(pid).unwrap();
    assert_eq!(source, Token::read(&original).unwrap());
    let ready = ready_event(pid);
    let running = child.resume().unwrap();
    assert_eq!(
        // SAFETY: bounded wait on the event owned by this fixture's resumed child.
        unsafe { WaitForSingleObject(ready.as_raw_handle(), 5000) },
        WAIT_OBJECT_0
    );
    assert!(!running.has_exited().unwrap());
    fixture.unavailable();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let stopped = runtime
        .block_on(super::super::launch::supervise(
            vec![running],
            std::future::pending(),
        ))
        .unwrap();
    assert_eq!(stopped, super::super::launch::Stopped::SessionChanged);
    assert_eq!(
        // SAFETY: waits only for the retained CI-created child; no PID lookup or arbitrary target.
        unsafe { WaitForSingleObject(process.as_raw_handle() as HANDLE, 5000) },
        WAIT_OBJECT_0
    );
    let fixture = Fixture::new();
    let role = Token::read(&user_token)
        .unwrap()
        .user_worker(session)
        .unwrap();
    let peer = Peer::worker(user.query_handle().unwrap(), &image, role, session).unwrap();
    let stale = SuspendedBroker::create(
        owner.peer().unwrap(),
        peer,
        fixture.binding(),
        &image,
        &format!("--exact {CHILD} --ignored"),
    )
    .unwrap();
    let stopped = stale.process.query_handle().unwrap();
    fixture.unavailable();
    assert!(stale.resume().is_err());
    assert_eq!(
        // SAFETY: the failed resume must terminate this retained suspended test process.
        unsafe { WaitForSingleObject(stopped.as_raw_handle(), 5000) },
        WAIT_OBJECT_0
    );
    drop(user);
    owner.finish().unwrap();
    println!(
        "SYSTEM companion: selected logon can query; mutation, other logon and stale binding refused"
    );
}
