use std::time::Duration;
use windows_sys::Win32::{
    Foundation::{ERROR_ACCESS_DENIED, HANDLE, WAIT_OBJECT_0},
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
            GetCurrentProcess, GetExitCodeProcess, OpenProcess, OpenProcessToken,
            PROCESS_CREATE_PROCESS, PROCESS_DUP_HANDLE, PROCESS_QUERY_INFORMATION,
            PROCESS_TERMINATE, PROCESS_VM_OPERATION, PROCESS_VM_READ, PROCESS_VM_WRITE,
            WaitForSingleObject,
        },
    },
};
use zeroize::Zeroizing;

use super::super::super::sessions::tests::Fixture;
use super::super::{identity::tests::local, startup::Broker};
use super::*;

const CHILD: &str = "win::remote_ipc::broker::tests::companion_child";
const USER_CHILD: &str = "win::remote_ipc::broker::tests::ordinary_bootstrap_child";
const PRIVILEGED_CHILD: &str = "win::remote_ipc::broker::tests::privileged_participant_child";
const CONSENT_CHILD: &str = "win::remote_ipc::broker::tests::consent_participant_child";

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

#[tokio::test]
#[ignore = "child fixture, launched only by its owning test"]
async fn companion_child() {
    let image = std::env::current_exe().unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    let (listener, launcher) = delivery::receive_companion(&image, deadline).await.unwrap();
    let mut connection = listener.accept(deadline).await.unwrap();
    assert_eq!(
        &*connection.receive(deadline).await.unwrap(),
        b"worker ready"
    );
    connection.send(b"companion ready", deadline).await.unwrap();
    launcher.connected(deadline).await.unwrap();
    tokio::time::sleep(Duration::from_secs(60)).await;
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

fn user_process(
    token: &OwnedHandle,
    image: &Path,
    launch_id: uuid::Uuid,
    arguments: &str,
) -> Process {
    let environment = super::super::bootstrap::delivery::worker_environment(launch_id).unwrap();
    user_process_in(token, image, &environment, arguments)
}

fn user_process_in(
    token: &OwnedHandle,
    image: &Path,
    environment: &[u16],
    arguments: &str,
) -> Process {
    let application = image_path(image).unwrap();
    let mut command = super::super::super::wide(&format!("\"{}\" {arguments}", image.display()));
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

async fn release_participants(parent: &Peer, token: &OwnedHandle, session: u32, image: &Path) {
    for reachable in [false, true] {
        let id = uuid::Uuid::new_v4();
        let worker = user_process(token, image, id, "remote-worker");
        let worker_process = worker.query_handle().unwrap();
        let role = Token::read(token).unwrap().user_worker(session).unwrap();
        let peer = Peer::worker(worker.query_handle().unwrap(), image, role, session).unwrap();
        let fixture = Fixture::new();
        let binding = fixture.binding();
        let broker = SuspendedBroker::create(
            parent,
            peer.duplicate().unwrap(),
            binding.clone(),
            image,
            "remote-broker",
        )
        .unwrap();
        let broker_process = broker.process.query_handle().unwrap();
        let broker_peer = broker.peer().unwrap();
        let (worker, handle) = worker.transfer_peer(&broker_peer).unwrap();
        let sender = delivery::Sender::bind(id, peer).unwrap();
        let endpoint = uuid::Uuid::new_v4();
        let started = std::time::Instant::now();
        let deadline = tokio::time::Instant::now() + super::super::STARTUP_TIMEOUT;
        let listening = broker.listen(endpoint, deadline).await.unwrap();
        let companion_ready = started.elapsed();
        let worker = RunningWorker::bound(worker.resume().unwrap(), binding.clone()).unwrap();
        let destination = if reachable {
            endpoint
        } else {
            uuid::Uuid::new_v4()
        };
        let result = sender
            .send(handle, destination, &binding, &broker_peer, deadline)
            .await;
        if !reachable {
            assert!(
                result.is_err(),
                "worker confirmed an unavailable control endpoint"
            );
            assert!(
                listening.connected().await.is_err(),
                "broker confirmed a missing worker"
            );
            continue;
        }
        let worker_receipt = result.unwrap_or_else(|error| {
            panic!(
                "agent worker must confirm the control connection: {error} after {:?} (companion ready after {companion_ready:?}, worker {}, companion {})",
                started.elapsed(),
                process_state(&worker_process),
                process_state(&broker_process)
            )
        });
        let worker_confirmed = started.elapsed();
        assert!(
            matches!(worker_receipt.connected(deadline).await, Err(e) if e.kind() == io::ErrorKind::PermissionDenied)
        );
        // The control channel stays open, as in the service, until the test ends the pair.
        let (broker, mut control) = listening
            .connected()
            .await
            .expect("independent companion confirmation");
        println!(
            "release pair: companion ready after {companion_ready:?}, worker confirmed after {worker_confirmed:?}, companion confirmed after {:?}",
            started.elapsed()
        );
        assert!(!broker.has_exited().unwrap() && !worker.has_exited().unwrap());
        // The SYSTEM companion is the session's WebRTC transport: it answers a browser offer.
        let host = std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST);
        let offered = std::time::Instant::now();
        let reply = control
            .exchange(
                &super::super::media::offer_message(
                    &[host],
                    None,
                    &browser_offer(),
                    midas_remote_auth::Transfer::NONE,
                ),
                tokio::time::Instant::now() + Duration::from_secs(5),
            )
            .await
            .expect("companion reply to the offer");
        let answer = super::super::media::read_answer(&reply).expect("an SDP answer");
        assert!(
            answer.contains("m=video") && answer.contains("a=candidate"),
            "{answer}"
        );
        println!(
            "release companion answered the offer after {:?}",
            offered.elapsed()
        );
        let again = control
            .exchange(
                &super::super::media::offer_message(
                    &[host],
                    None,
                    &browser_offer(),
                    midas_remote_auth::Transfer::NONE,
                ),
                tokio::time::Instant::now() + Duration::from_secs(5),
            )
            .await
            .unwrap();
        assert_eq!(
            super::super::media::read_answer(&again),
            None,
            "one transport per session"
        );
        assert!(!broker.has_exited().unwrap() && !worker.has_exited().unwrap());
        // With UAC rights the session's privileged worker joins the companion, once.
        let observer = std::sync::Arc::new(crate::win::sessions::Observer::default());
        let target = observer
            .collect()
            .expect("native sessions")
            .into_iter()
            .find(|candidate| candidate.id == session)
            .expect("console session");
        // The observer outlives the worker, as in the service: its bindings end with it.
        let token =
            super::super::tokens::PreparedToken::privileged(parent, observer.clone(), target)
                .expect("privileged token for the console session");
        let joining = super::super::launch::SuspendedWorker::spawn(parent, token, image).unwrap();
        let joined = std::time::Instant::now();
        let privileged = super::super::bootstrap::attach_privileged(
            parent,
            &broker_peer,
            &broker,
            &mut control,
            joining,
            tokio::time::Instant::now() + super::super::STARTUP_TIMEOUT,
        )
        .await
        .unwrap_or_else(|error| {
            panic!(
                "privileged worker must join the companion: {error} (companion {})",
                process_state(&broker_process)
            )
        });
        println!(
            "release privileged worker joined the companion after {:?}",
            joined.elapsed()
        );
        let again = control
            .exchange(
                &super::super::media::listen_message(4, uuid::Uuid::new_v4()),
                tokio::time::Instant::now() + Duration::from_secs(5),
            )
            .await
            .unwrap();
        assert!(
            !super::super::media::listening(&again),
            "one privileged worker per session"
        );
        assert!(!broker.has_exited().unwrap() && !privileged.has_exited().unwrap());
        drop(worker);
        assert_eq!(
            // SAFETY: bounded wait on the retained CI companion while its own job is still open.
            unsafe { WaitForSingleObject(broker_process.as_raw_handle(), 5000) },
            WAIT_OBJECT_0
        );
        let mut code = u32::MAX;
        assert_ne!(
            // SAFETY: the retained handle belongs to the exited companion and code is writable.
            unsafe { GetExitCodeProcess(broker_process.as_raw_handle(), &mut code) },
            0
        );
        assert_eq!(
            code, 0,
            "companion should exit cleanly after worker disconnect"
        );
        for _ in 0..50 {
            if privileged.has_exited().unwrap() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(
            privileged.has_exited().unwrap(),
            "privileged worker should stop with its companion"
        );
    }
}

/// What a browser offers: receive-only video and a control data channel.
fn browser_offer() -> String {
    str0m::crypto::from_feature_flags().install_process_default();
    let mut browser = str0m::RtcConfig::new()
        .clear_codecs()
        .enable_h264(true)
        .build(std::time::Instant::now());
    let mut change = browser.sdp_api();
    change.add_media(
        str0m::media::MediaKind::Video,
        str0m::media::Direction::RecvOnly,
        None,
        None,
        None,
    );
    change.add_channel("control".into());
    let (offer, _) = change.apply().unwrap();
    offer.to_sdp_string()
}

fn process_state(process: &OwnedHandle) -> String {
    let mut code = u32::MAX;
    // SAFETY: the retained process handle is open and code is writable.
    if unsafe { GetExitCodeProcess(process.as_raw_handle(), &mut code) } == 0 {
        return format!("state unknown: {}", io::Error::last_os_error());
    }
    // STILL_ACTIVE: the process has not exited.
    if code == 259 {
        "still running".into()
    } else {
        format!("exited with {code:#x}")
    }
}

#[test]
#[ignore = "privileged child fixture, launched only by the isolated SYSTEM trial"]
fn privileged_participant_child() {
    super::super::participant::Participant::Privileged
        .run()
        .unwrap();
}

/// A SYSTEM worker in the console session connects to the service's own control endpoint and
/// stops when the service closes it.
async fn privileged_participant(parent: &Peer, session: u32, image: &Path) {
    let observer = std::sync::Arc::new(crate::win::sessions::Observer::default());
    let target = observer
        .collect()
        .expect("native sessions")
        .into_iter()
        .find(|candidate| candidate.id == session)
        .expect("console session");
    let described = format!(
        "session {}, presence {}, locked {:?}, user present {}",
        target.id,
        target.presence,
        target.locked,
        !target.account.is_empty()
    );
    println!("privileged target: {described}");
    let token = super::super::tokens::PreparedToken::privileged(parent, observer.clone(), target)
        .unwrap_or_else(|error| panic!("privileged token for {described}: {error}"));
    let worker = super::super::launch::SuspendedWorker::spawn_fixture(
        parent,
        token,
        image,
        &format!("--exact {PRIVILEGED_CHILD} --ignored"),
    )
    .unwrap();
    let process = worker.peer().unwrap();
    let started = std::time::Instant::now();
    let deadline = tokio::time::Instant::now() + super::super::STARTUP_TIMEOUT;
    let (running, connection) = super::super::bootstrap::start_privileged(parent, worker, deadline)
        .await
        .unwrap_or_else(|error| {
            panic!(
                "privileged worker must connect to the service: {error} after {:?}",
                started.elapsed()
            )
        });
    println!(
        "privileged worker connected to the service after {:?}",
        started.elapsed()
    );
    process.recheck().unwrap();
    assert!(!running.has_exited().unwrap());
    drop(connection);
    for _ in 0..50 {
        if running.has_exited().unwrap() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        running.has_exited().unwrap(),
        "privileged worker should stop after the service closes its channel"
    );
}

#[test]
#[ignore = "consent child fixture, launched only by the isolated SYSTEM trial"]
fn consent_participant_child() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    // The real participant protocol; only the windows are replaced: the user allows, then
    // presses End on the session indicator.
    let result = runtime.block_on(super::super::participant::consent(
        tokio::time::Instant::now() + super::super::STARTUP_TIMEOUT,
        |question| {
            assert_eq!(question.operator, "Trial Operator");
            assert_eq!(question.customer, "Trial Customer");
            assert!(question.uac && !question.administrative);
            Ok(crate::consent::Answer::Allow)
        },
        |question, _| {
            assert_eq!(question.operator, "Trial Operator");
            Ok(true)
        },
    ));
    if let Err(error) = result {
        eprintln!("consent child: {error}");
        std::process::exit(super::super::participant::failure_code().into());
    }
}

/// The consent window runs as the selected user in the console session, receives no handle to
/// the service, and answers over its verified startup pipe, also after the I/O limit.
async fn consent_participant(parent: &Peer, token: &OwnedHandle, session: u32, image: &Path) {
    use crate::consent::{Answer, Question, decode_answer};
    let id = uuid::Uuid::new_v4();
    let environment = super::super::bootstrap::delivery::consent_environment(id).unwrap();
    let window = user_process_in(
        token,
        image,
        &environment,
        &format!("--exact {CONSENT_CHILD} --ignored"),
    );
    let process = window.query_handle().unwrap();
    let role = Token::read(token).unwrap().user_worker(session).unwrap();
    let peer = Peer::worker(window.query_handle().unwrap(), image, role, session).unwrap();
    let fixture = Fixture::new();
    let binding = fixture.binding();
    let sender = delivery::Sender::for_consent(id, peer).unwrap();
    let deadline = tokio::time::Instant::now() + super::super::STARTUP_TIMEOUT;
    let started = std::time::Instant::now();
    let running = RunningWorker::bound(window.resume().unwrap(), binding.clone()).unwrap();
    let sent = sender
        .send(0, uuid::Uuid::nil(), &binding, parent, deadline)
        .await
        .unwrap_or_else(|error| {
            panic!(
                "consent window must receive its startup record: {error} ({})",
                process_state(&process)
            )
        });
    let mut control = sent.connected(deadline).await.unwrap_or_else(|error| {
        panic!(
            "consent window must confirm its channel: {error} ({})",
            process_state(&process)
        )
    });
    let request = uuid::Uuid::new_v4();
    let question = Question {
        request,
        operator: "Trial Operator".into(),
        customer: "Trial Customer".into(),
        administrative: false,
        uac: true,
        transfer: midas_remote_auth::Transfer::NONE,
        seconds: 30,
    };
    let answer = control
        .ask(&question.encode().unwrap(), deadline)
        .await
        .unwrap_or_else(|error| {
            panic!(
                "consent window must answer: {error} ({})",
                process_state(&process)
            )
        });
    assert_eq!(decode_answer(&answer), Some((request, Answer::Allow)));
    println!(
        "consent window answered the service after {:?}",
        started.elapsed()
    );
    control
        .send(
            crate::consent::INDICATE,
            tokio::time::Instant::now() + Duration::from_secs(5),
        )
        .await
        .unwrap();
    let ended = control
        .exchange(
            crate::consent::COLLECT,
            tokio::time::Instant::now() + Duration::from_secs(5),
        )
        .await
        .unwrap();
    assert_eq!(ended, crate::consent::END);
    println!("session indicator sent the user's End");
    drop(control);
    for _ in 0..50 {
        if running.has_exited().unwrap() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(
        process_state(&process),
        "exited with 0x0",
        "consent window should stop cleanly after the service closes its channel"
    );
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
        let mut expected_endpoint = [0; 16];
        pipe.read_exact(&mut expected_endpoint).await.unwrap();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        let delivery: io::Result<()> = async {
            let received = delivery::receive(&image, deadline).await?;
            received.verify_fixture(
                outcome.as_ref().map_err(|_| denied())?,
                uuid::Uuid::from_bytes(expected_endpoint),
            )?;
            let mut connection = received.connect()?;
            connection.send(b"worker ready", deadline).await?;
            if &*connection.receive(deadline).await? != b"companion ready" {
                return Err(denied());
            }
            Ok(())
        }
        .await;
        let message = match &outcome {
            Ok(_) if delivery.is_err() => format!("metadata delivery failed: {delivery:?}"),
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
    let launch_id = uuid::Uuid::new_v4();
    let user_args = format!("--exact {USER_CHILD} --ignored");
    let user = user_process(&user_token, &image, launch_id, &user_args);
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
    let wrong = user_process(&user_token, &image, uuid::Uuid::new_v4(), &user_args);
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
    runtime.block_on(async {
        let listener =
            super::super::Listener::bind(owner.peer().unwrap(), user_peer.duplicate().unwrap())
                .unwrap();
        let occupied = super::super::endpoint_id(listener.name()).unwrap();
        let blocked = SuspendedBroker::create(
            owner.peer().unwrap(),
            user_peer.duplicate().unwrap(),
            fixture.binding(),
            &image,
            &format!("--exact {CHILD} --ignored"),
        )
        .unwrap();
        let process = blocked.process.query_handle().unwrap();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        assert!(blocked.listen(occupied, deadline).await.is_err());
        assert_eq!(
            // SAFETY: waits for this fixture's own companion after failed startup.
            unsafe { WaitForSingleObject(process.as_raw_handle(), 5000) },
            WAIT_OBJECT_0
        );
        user_peer.recheck().unwrap();
        drop(listener);
    });
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
    let endpoint = uuid::Uuid::new_v4();
    let sender = runtime.block_on(async {
        super::super::bootstrap::delivery::Sender::bind(launch_id, user_peer.duplicate().unwrap())
            .unwrap()
    });
    let listening = runtime
        .block_on(async {
            child
                .listen(
                    endpoint,
                    tokio::time::Instant::now() + Duration::from_secs(5),
                )
                .await
        })
        .expect("companion must bind its verified worker's endpoint before user startup");
    let user = user.resume().unwrap();
    runtime.block_on(async {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        tokio::time::timeout(Duration::from_secs(5), async {
            bootstrap_pipe.connect().await.unwrap();
            user_peer
                .check(super::super::peer_pid(&bootstrap_pipe, true).unwrap())
                .unwrap();
            bootstrap_pipe.write_u64_le(handle_value).await.unwrap();
            bootstrap_pipe.write_all(endpoint.as_bytes()).await.unwrap();
            let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
            let binding = fixture.binding();
            let (sent, length) = tokio::join!(
                sender.send(handle_value, endpoint, &binding, &pinned, deadline),
                bootstrap_pipe.read_u32_le(),
            );
            let length = length.unwrap() as usize;
            assert!(length > 0 && length <= 1024);
            let mut reply = vec![0u8; length];
            bootstrap_pipe.read_exact(&mut reply).await.unwrap();
            assert_eq!(String::from_utf8_lossy(&reply), "verified");
            sent.expect("system metadata sender");
            user_peer.recheck().unwrap();
        })
        .await
        .unwrap_or_else(|_| {
            panic!(
                "ordinary worker did not confirm startup: {}",
                process_state(&user_handle)
            )
        });
    });
    println!(
        "ordinary worker imported the query-only companion handle without elevation or inheritance"
    );
    println!("paired worker and companion exchanged authenticated control frames");
    let (running, _control) = runtime.block_on(listening.connected()).unwrap();
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
    runtime.block_on(release_participants(
        owner.peer().unwrap(),
        &user_token,
        session,
        &image.with_file_name("midas-agent.exe"),
    ));
    println!(
        "release agent pair confirmed readiness, refused a missing endpoint and stopped on disconnect"
    );
    runtime.block_on(privileged_participant(
        owner.peer().unwrap(),
        session,
        &image,
    ));
    println!(
        "SYSTEM worker in the console session connected only to the service and stopped with it"
    );
    runtime.block_on(consent_participant(
        owner.peer().unwrap(),
        &user_token,
        session,
        &image,
    ));
    println!("consent window ran as the selected user and answered only the service");
    owner.finish().unwrap();
    println!(
        "SYSTEM companion: selected logon can query; mutation, other logon and stale binding refused"
    );
}
