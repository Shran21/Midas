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
const USER_CHILD: &str = "win::remote_ipc::broker::tests::ordinary_bootstrap_child";

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
    let mut command = super::super::super::wide(&format!(
        "\"{}\" --exact {USER_CHILD} --ignored",
        image.display()
    ));
    let environment = environment().unwrap();
    let mut desktop = [0u16];
    Process::create(Some(&mut desktop), |startup, flags, information| {
        // SAFETY: this CI-only standard-user fixture starts suspended; all buffers outlive creation.
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

fn bootstrap_name(pid: u32) -> String {
    format!(r"\\.\pipe\midas-test-bootstrap-{pid}")
}

#[test]
#[ignore = "ordinary child fixture, launched only by the isolated SYSTEM trial"]
fn ordinary_bootstrap_child() {
    use std::{fs::OpenOptions, os::windows::fs::OpenOptionsExt};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use windows_sys::Win32::{
        Foundation::{GetHandleInformation, HANDLE_FLAG_INHERIT},
        Storage::FileSystem::{FILE_FLAG_OVERLAPPED, SECURITY_IDENTIFICATION},
        System::{RemoteDesktop::ProcessIdToSessionId, Threading::TerminateProcess},
    };

    let image = std::env::current_exe().unwrap();
    let token = current_token(TOKEN_QUERY).unwrap();
    let before = Token::read(&token).unwrap();
    let mut session = 0;
    assert_ne!(
        // SAFETY: queries this fixture's own native session into a writable output.
        unsafe { ProcessIdToSessionId(std::process::id(), &mut session) },
        0
    );
    before.user_worker(session).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let file = OpenOptions::new()
            .access_mode(super::super::CLIENT_ACCESS)
            .custom_flags(FILE_FLAG_OVERLAPPED)
            .security_qos_flags(SECURITY_IDENTIFICATION)
            .open(bootstrap_name(std::process::id()))
            .unwrap();
        let mut pipe = super::super::super::into_pipe_client(file).unwrap();
        let value = tokio::time::timeout(Duration::from_secs(5), pipe.read_u64_le())
            .await
            .unwrap()
            .unwrap();
        let outcome = std::panic::catch_unwind(|| {
            let mut flags = 0;
            assert_ne!(
                // SAFETY: queries the live handle installed by the SYSTEM test launcher, without owning it.
                unsafe { GetHandleInformation(value as usize as HANDLE, &mut flags) },
                0
            );
            assert_eq!(flags & HANDLE_FLAG_INHERIT, 0);
            assert!(
                super::super::bootstrap::import_broker(value, &image.with_file_name("wrong.exe"))
                    .is_err()
            );
            let peer = super::super::bootstrap::import_broker(value, &image).unwrap();
            peer.recheck().unwrap();
            assert_ne!(
                // SAFETY: failed and successful imports must both leave the original handle alive.
                unsafe { GetHandleInformation(value as usize as HANDLE, &mut flags) },
                0
            );
            let handle = peer.process_handle().unwrap();
            assert_ne!(
                // SAFETY: queries the independently owned, imported process handle.
                unsafe { GetHandleInformation(handle.as_raw_handle(), &mut flags) },
                0
            );
            assert_eq!(flags & HANDLE_FLAG_INHERIT, 0);
            for rights in [PROCESS_TERMINATE, PROCESS_DUP_HANDLE, PROCESS_VM_WRITE] {
                refused(duplicate_access(handle, rights));
            }
            assert_eq!(
                // SAFETY: a query-only handle must not allow this CI fixture to terminate its companion.
                unsafe { TerminateProcess(handle.as_raw_handle(), 1) },
                0
            );
            assert_eq!(
                io::Error::last_os_error().raw_os_error(),
                Some(ERROR_ACCESS_DENIED as i32)
            );
            assert_eq!(before, Token::read(&token).unwrap());
            peer
        });
        let message = match &outcome {
            Ok(_) => "verified".to_owned(),
            Err(error) => error
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| error.downcast_ref::<&str>().map(|text| (*text).to_owned()))
                .unwrap_or_else(|| "native import assertion failed".to_owned()),
        };
        let bytes = message.as_bytes();
        let bytes = &bytes[..bytes.len().min(1024)];
        pipe.write_u32_le(bytes.len() as u32).await.unwrap();
        pipe.write_all(bytes).await.unwrap();
        assert!(outcome.is_ok(), "ordinary bootstrap verification failed");
        let _ = pipe.read_u8().await;
    });
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

fn duplicate_access(source: &OwnedHandle, rights: u32) -> io::Result<OwnedHandle> {
    let mut raw = ptr::null_mut();
    // SAFETY: source is a retained process object; no inheritance or source close is requested.
    if unsafe {
        windows_sys::Win32::Foundation::DuplicateHandle(
            GetCurrentProcess(),
            source.as_raw_handle(),
            GetCurrentProcess(),
            &mut raw,
            rights,
            0,
            0,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: only the newly returned, independently owned process handle is adopted.
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
    let user_peer = child.preparation.user.duplicate().unwrap();
    child
        .check_worker(owner.peer().unwrap(), &user_peer)
        .unwrap();
    let wrong = user_process(&user_token, &image);
    let wrong_peer = Peer::worker(
        wrong.query_handle().unwrap(),
        &image,
        Token::read(&user_token)
            .unwrap()
            .user_worker(session)
            .unwrap(),
        session,
    )
    .unwrap();
    assert!(
        child
            .check_worker(owner.peer().unwrap(), &wrong_peer)
            .is_err()
    );
    drop(wrong);
    let (user, handle_value) = user.transfer_peer(&pinned).unwrap();
    let user_handle = user.query_handle().unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mut bootstrap_pipe = runtime.block_on(async {
        let mut options = tokio::net::windows::named_pipe::ServerOptions::new();
        options
            .first_pipe_instance(true)
            .reject_remote_clients(true)
            .max_instances(1);
        super::super::super::create_pipe(
            &options,
            &bootstrap_name(user.pid().unwrap()),
            &user_peer.acl().unwrap(),
        )
        .unwrap()
    });
    let user = user.resume().unwrap();
    runtime.block_on(async {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        tokio::time::timeout(Duration::from_secs(5), async {
            bootstrap_pipe.connect().await.unwrap();
            user_peer
                .check(super::super::peer_pid(&bootstrap_pipe, true).unwrap())
                .unwrap();
            bootstrap_pipe.write_u64_le(handle_value).await.unwrap();
            let length = bootstrap_pipe.read_u32_le().await.unwrap() as usize;
            assert!(length > 0 && length <= 1024);
            let mut reply = vec![0u8; length];
            bootstrap_pipe.read_exact(&mut reply).await.unwrap();
            assert_eq!(String::from_utf8_lossy(&reply), "verified");
            user_peer.recheck().unwrap();
        })
        .await
        .unwrap();
    });
    println!(
        "ordinary worker imported the query-only companion handle without elevation or inheritance"
    );
    let ready = ready_event(pid);
    let running = child.resume().unwrap();
    assert_eq!(
        // SAFETY: bounded wait on the event owned by this fixture's resumed child.
        unsafe { WaitForSingleObject(ready.as_raw_handle(), 5000) },
        WAIT_OBJECT_0
    );
    assert!(!running.has_exited().unwrap());
    let stale_fixture = Fixture::new();
    let role = Token::read(&user_token)
        .unwrap()
        .user_worker(session)
        .unwrap();
    let peer = Peer::worker(user.query_handle().unwrap(), &image, role, session).unwrap();
    let stale = SuspendedBroker::create(
        owner.peer().unwrap(),
        peer,
        stale_fixture.binding(),
        &image,
        &format!("--exact {CHILD} --ignored"),
    )
    .unwrap();
    let stopped = stale.process.query_handle().unwrap();
    stale_fixture.unavailable();
    assert!(stale.resume().is_err());
    assert_eq!(
        // SAFETY: the failed resume must terminate this retained suspended test process.
        unsafe { WaitForSingleObject(stopped.as_raw_handle(), 5000) },
        WAIT_OBJECT_0
    );
    let user = RunningWorker::bound(user, fixture.binding()).unwrap();
    fixture.unavailable();
    let stopped = runtime
        .block_on(super::super::launch::supervise(
            vec![running, user],
            std::future::pending(),
        ))
        .unwrap();
    assert_eq!(stopped, super::super::launch::Stopped::SessionChanged);
    for handle in [&process, &user_handle] {
        assert_eq!(
            // SAFETY: waits only for the retained CI-created workers; no PID lookup or arbitrary target.
            unsafe { WaitForSingleObject(handle.as_raw_handle(), 5000) },
            WAIT_OBJECT_0
        );
    }
    drop(bootstrap_pipe);
    owner.finish().unwrap();
    println!(
        "SYSTEM companion: selected logon can query; mutation, other logon and stale binding refused"
    );
}
