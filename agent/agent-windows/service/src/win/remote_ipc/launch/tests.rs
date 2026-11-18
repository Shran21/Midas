use std::{
    fs,
    os::windows::process::CommandExt,
    path::PathBuf,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use windows_sys::Win32::System::{
    JobObjects::QueryInformationJobObject,
    Threading::{CreateProcessW, SuspendThread},
};

use super::*;

use super::super::super::sessions::tests::Fixture;

fn running(process: Process) -> RunningWorker {
    static FIXTURE: std::sync::OnceLock<Fixture> = std::sync::OnceLock::new();
    RunningWorker {
        process,
        session: FIXTURE.get_or_init(Fixture::new).binding(),
    }
}

const CHILD: &str = "win::remote_ipc::launch::tests::worker_child";
const OWNER: &str = "win::remote_ipc::launch::tests::owner_child";

struct Output(PathBuf);

impl Output {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("midas-launch-{}.txt", uuid::Uuid::new_v4())))
    }

    fn wait(&self) -> String {
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            if let Ok(value) = fs::read_to_string(&self.0)
                && let Some(value) = value.strip_suffix('\n')
            {
                return value.into();
            }
            assert!(Instant::now() < until, "child did not report startup");
            thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn create_child(output: &Output, mode: &str) -> Process {
    let image = image_path(&std::env::current_exe().unwrap()).unwrap();
    let mut line = vec![b'"' as u16];
    line.extend_from_slice(&image[..image.len() - 1]);
    line.extend(format!("\" --exact {CHILD} --nocapture").encode_utf16());
    line.push(0);
    let mut env = environment().unwrap();
    env.pop();
    for (key, value) in [
        ("MIDAS_LAUNCH_CHILD", mode.into()),
        ("MIDAS_LAUNCH_OUTPUT", output.0.as_os_str().to_owned()),
    ] {
        env.extend(format!("{key}=").encode_utf16());
        env.extend(value.encode_wide());
        env.push(0);
    }
    env.push(0);
    Process::create(None, |startup, flags, information| {
        // SAFETY: owned string buffers and startup attributes outlive this same-user test creation.
        unsafe {
            CreateProcessW(
                image.as_ptr(),
                line.as_mut_ptr(),
                ptr::null(),
                ptr::null(),
                0,
                flags,
                env.as_ptr().cast(),
                ptr::null(),
                startup,
                information,
            )
        }
    })
    .unwrap()
}

fn stopped(process: &OwnedHandle) {
    assert_eq!(
        // SAFETY: waits on this test's retained spawned-process handle, with a bounded timeout.
        unsafe { WaitForSingleObject(process.as_raw_handle(), 5000) },
        WAIT_OBJECT_0
    );
}

#[test]
fn worker_child() {
    let Ok(mode) = std::env::var("MIDAS_LAUNCH_CHILD") else {
        return;
    };
    let output = std::env::var_os("MIDAS_LAUNCH_OUTPUT").unwrap();
    let result = if mode == "spawn" {
        let child = Command::new(std::env::current_exe().unwrap())
            .arg("--list")
            .creation_flags(CREATE_NO_WINDOW)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        match child {
            Ok(mut child) => {
                let _ = child.kill();
                let _ = child.wait();
                "escaped"
            }
            Err(_) => "blocked",
        }
    } else {
        "started"
    };
    fs::write(output, format!("{result}\n")).unwrap();
    if mode == "exit" {
        return;
    }
    thread::sleep(Duration::from_secs(20));
}

#[tokio::test]
async fn native_exit_wait_handles_cancellation_and_already_exited_processes() {
    let output = Output::new();
    let child = create_child(&output, "wait");
    let process = child.query_handle().unwrap();
    for _ in 0..32 {
        drop(wait::ExitWait::new(child.query_handle().unwrap()).unwrap());
    }
    assert!(!child.has_exited().unwrap());
    let wait = wait::ExitWait::new(child.query_handle().unwrap()).unwrap();
    let waiting = tokio::spawn(wait.wait());
    drop(child);
    tokio::time::timeout(Duration::from_secs(5), waiting)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    stopped(&process);
    tokio::time::timeout(
        Duration::from_secs(5),
        wait::ExitWait::new(process).unwrap().wait(),
    )
    .await
    .unwrap()
    .unwrap();
}

#[tokio::test]
async fn cancelling_a_ready_native_callback_keeps_its_context_alive() {
    let output = Output::new();
    let child = create_child(&output, "wait");
    let process = child.query_handle().unwrap();
    drop(child);
    stopped(&process);
    for _ in 0..64 {
        let wait = wait::ExitWait::new(process.try_clone().unwrap()).unwrap();
        let task = tokio::spawn(wait.wait());
        tokio::task::yield_now().await;
        task.abort();
        match tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
        {
            Ok(result) => result.unwrap(),
            Err(error) => assert!(error.is_cancelled()),
        }
    }
    tokio::time::timeout(
        Duration::from_secs(2),
        wait::ExitWait::new(process).unwrap().wait(),
    )
    .await
    .unwrap()
    .unwrap();
}

#[tokio::test]
async fn native_session_loss_terminates_workers_and_their_companions() {
    for cause in 0..4 {
        for before_poll in [true, false] {
            let fixture = Fixture::new();
            let other = Fixture::new();
            let outputs = [Output::new(), Output::new()];
            let workers = vec![
                RunningWorker {
                    process: create_child(&outputs[0], "wait").resume().unwrap(),
                    session: fixture.binding(),
                },
                RunningWorker {
                    process: create_child(&outputs[1], "wait").resume().unwrap(),
                    session: other.binding(),
                },
            ];
            let processes: Vec<_> = workers
                .iter()
                .map(|w| w.process.query_handle().unwrap())
                .collect();
            let supervision = supervise(workers, std::future::pending());
            let (start, started) = tokio::sync::oneshot::channel();
            let mut start = Some(start);
            let task = tokio::spawn(async move {
                started.await.unwrap();
                supervision.await
            });
            if !before_poll {
                start.take().unwrap().send(()).unwrap();
                for output in &outputs {
                    assert_eq!(output.wait(), "started");
                }
                tokio::task::yield_now().await;
            }
            match cause {
                0 => fixture.observer.changed(),
                1 => fixture.unavailable(),
                2 => fixture.observer.stop(),
                3 => drop(fixture),
                _ => unreachable!(),
            }
            if let Some(start) = start {
                start.send(()).unwrap();
            }
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(10), task)
                    .await
                    .unwrap()
                    .unwrap()
                    .unwrap(),
                Stopped::SessionChanged
            );
            other.binding().check().unwrap();
            for process in processes {
                stopped(&process);
            }
        }
    }
}

#[tokio::test]
async fn requested_stop_terminates_and_observes_all_workers() {
    let outputs = [Output::new(), Output::new()];
    let workers: Vec<_> = outputs
        .iter()
        .map(|output| running(create_child(output, "wait").resume().unwrap()))
        .collect();
    let processes: Vec<_> = workers
        .iter()
        .map(|w| w.process.query_handle().unwrap())
        .collect();
    let (stop, stopped_signal) = tokio::sync::oneshot::channel();
    let supervising = tokio::spawn(supervise(workers, async {
        let _ = stopped_signal.await;
    }));
    tokio::task::yield_now().await;
    stop.send(()).unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(10), supervising)
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
        Stopped::Requested
    );
    for process in processes {
        stopped(&process);
    }
}

#[tokio::test]
async fn one_workers_exit_terminates_its_remaining_companion() {
    let outputs = [Output::new(), Output::new()];
    let workers = vec![
        running(create_child(&outputs[0], "exit").resume().unwrap()),
        running(create_child(&outputs[1], "wait").resume().unwrap()),
    ];
    let processes: Vec<_> = workers
        .iter()
        .map(|w| w.process.query_handle().unwrap())
        .collect();
    assert_eq!(
        tokio::time::timeout(
            Duration::from_secs(10),
            supervise(workers, std::future::pending())
        )
        .await
        .unwrap()
        .unwrap(),
        Stopped::WorkerExited
    );
    for process in processes {
        stopped(&process);
    }
}

#[tokio::test]
async fn cancelling_supervision_kills_workers_even_before_its_first_poll() {
    for poll in [false, true] {
        let output = Output::new();
        let worker = running(create_child(&output, "wait").resume().unwrap());
        let process = worker.process.query_handle().unwrap();
        let supervision = supervise(vec![worker], std::future::pending());
        if poll {
            let task = tokio::spawn(supervision);
            tokio::task::yield_now().await;
            task.abort();
            assert!(task.await.unwrap_err().is_cancelled());
        } else {
            drop(supervision);
        }
        stopped(&process);
    }
}

#[tokio::test]
async fn an_immediate_stop_is_not_lost_and_empty_supervision_is_refused() {
    assert_eq!(
        supervise(vec![], std::future::ready(()))
            .await
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidInput
    );
    let output = Output::new();
    let worker = running(create_child(&output, "wait").resume().unwrap());
    let process = worker.process.query_handle().unwrap();
    assert_eq!(
        supervise(vec![worker], std::future::ready(()))
            .await
            .unwrap(),
        Stopped::Requested
    );
    stopped(&process);
}

#[tokio::test]
async fn signed_lease_revocation_channel_loss_and_expiry_stop_real_workers() {
    use midas_agent_core::remote::{Channel, DesktopKind, Observation, Presence};
    use midas_remote_auth::{Claims, Connection, Mode, Rights, SigningKey, Target};
    use uuid::Uuid;

    for reason in ["revoked", "disconnected", "expired"] {
        let key = SigningKey::generate().unwrap();
        let connection = Connection {
            tenant: Uuid::new_v4(),
            device: Uuid::new_v4(),
            id: Uuid::new_v4(),
        };
        let channel = Channel::from_authenticated_server(connection, key.public_key()).unwrap();
        let target = Target {
            connection,
            session: 4,
            generation: Uuid::new_v4(),
        };
        let rights = Rights {
            mode: Mode::Administrative,
            uac: true,
        };
        let operator = Uuid::new_v4();
        let started = Instant::now();
        let pending = channel.prepare(target, operator, rights).unwrap();
        let grant = key
            .sign(&Claims {
                id: Uuid::new_v4(),
                challenge: pending.challenge(),
                operator,
                target,
                rights,
                issued_at: 100,
                expires_at: if reason == "expired" { 101 } else { 130 },
            })
            .unwrap();
        let lease = pending
            .accept(
                &grant,
                Observation {
                    target,
                    presence: Presence::Vacant,
                    desktop: DesktopKind::User,
                    desktop_epoch: Uuid::new_v4(),
                },
            )
            .unwrap();
        let cancel = lease.cancellation();
        let output = Output::new();
        let worker = running(create_child(&output, "wait").resume().unwrap());
        let process = worker.process.query_handle().unwrap();
        let supervising = tokio::spawn(async move { supervise(vec![worker], lease.ended()).await });
        tokio::task::yield_now().await;
        match reason {
            "revoked" => cancel.revoke(),
            "disconnected" => drop(channel),
            _ => {}
        }
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(10), supervising)
                .await
                .unwrap()
                .unwrap()
                .unwrap(),
            Stopped::Requested
        );
        if reason == "expired" {
            assert!(started.elapsed() >= Duration::from_secs(1));
        }
        stopped(&process);
    }
}

#[test]
fn suspended_worker_cannot_run_and_is_killed_when_dropped() {
    let output = Output::new();
    let child = create_child(&output, "wait");
    let process = child.query_handle().unwrap();
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    assert_ne!(
        // SAFETY: reads the owned job's complete limits into an aligned structure.
        unsafe {
            QueryInformationJobObject(
                child.job.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                ptr::from_mut(&mut limits).cast(),
                size_of_val(&limits) as u32,
                ptr::null_mut(),
            )
        },
        0
    );
    assert_eq!(
        limits.BasicLimitInformation.LimitFlags,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_ACTIVE_PROCESS
    );
    assert_eq!(limits.BasicLimitInformation.ActiveProcessLimit, 1);
    assert_eq!(limits.ProcessMemoryLimit, 0);
    assert_eq!(limits.JobMemoryLimit, 0);
    assert!(!child.has_exited().unwrap());
    assert!(!output.0.exists());
    drop(child);
    stopped(&process);
    assert!(!output.0.exists());
}

#[test]
fn running_worker_is_contained_and_stops_on_explicit_termination() {
    let output = Output::new();
    let child = running(create_child(&output, "spawn").resume().unwrap());
    let process = child.process.query_handle().unwrap();
    assert_eq!(output.wait(), "blocked");
    assert!(!child.has_exited().unwrap());
    child.terminate().unwrap();
    stopped(&process);
    assert!(child.has_exited().unwrap());
}

#[test]
fn unexpected_suspend_count_and_second_resume_kill_the_worker() {
    let output = Output::new();
    let child = create_child(&output, "wait");
    let process = child.query_handle().unwrap();
    assert_eq!(
        // SAFETY: only the test owner holds this suspended primary-thread handle.
        unsafe { SuspendThread(child.thread.as_ref().unwrap().as_raw_handle()) },
        1
    );
    assert!(child.resume().is_err());
    stopped(&process);
    assert!(!output.0.exists());

    let child = create_child(&output, "wait").resume().unwrap();
    let process = child.query_handle().unwrap();
    assert!(child.resume().is_err());
    stopped(&process);
}

#[test]
fn launch_rejects_ambiguous_paths_and_inherits_no_parent_environment() {
    for path in [
        "worker.exe",
        r"C:worker.exe",
        r"\worker.exe",
        r"\\host\share\worker.exe",
        r"\\?\C:\worker.exe",
        r"C:\folder\..\worker.exe",
        r"C:\worker.exe:payload",
        "C:\\bad\"name.exe",
        "C:\\bad\0name.exe",
    ] {
        assert!(image_path(Path::new(path)).is_err(), "{path:?}");
    }
    assert!(image_path(Path::new(r"C:\Program Files\MIDAS\worker.exe")).is_ok());
    let vars = String::from_utf16(&environment().unwrap()).unwrap();
    let vars: Vec<_> = vars.split('\0').filter(|v| !v.is_empty()).collect();
    assert_eq!(vars.len(), 3);
    assert!(vars.iter().all(|v| {
        ["PATH=", "SystemRoot=", "WINDIR="]
            .iter()
            .any(|key| v.starts_with(key))
    }));
}

#[test]
fn failed_process_creation_returns_the_original_error() {
    let result = Process::create(None, |_, _, _| {
        // SAFETY: changes only this test thread's last-error slot for a simulated Win32 failure.
        unsafe {
            windows_sys::Win32::Foundation::SetLastError(2);
        }
        0
    });
    assert!(matches!(result, Err(error) if error.raw_os_error() == Some(2)));
}

#[test]
fn owner_child() {
    let Some(path) = std::env::var_os("MIDAS_LAUNCH_OWNER") else {
        return;
    };
    let output = Output(PathBuf::from(&path).with_extension("worker"));
    let child = create_child(&output, "wait");
    let child = if std::env::var("MIDAS_LAUNCH_RESUME").unwrap() == "1" {
        let child = child.resume().unwrap();
        assert_eq!(output.wait(), "started");
        child
    } else {
        child
    };
    fs::write(path, format!("{}\n", child.pid().unwrap())).unwrap();
    thread::sleep(Duration::from_secs(20));
}

struct Owner(std::process::Child);

impl Drop for Owner {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn owner_crash_kills_suspended_and_running_workers() {
    for resume in ["0", "1"] {
        let output = Output::new();
        let _worker_output = Output(output.0.with_extension("worker"));
        let mut owner = Owner(
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", OWNER, "--nocapture"])
                .creation_flags(CREATE_NO_WINDOW)
                .env("MIDAS_LAUNCH_OWNER", &output.0)
                .env("MIDAS_LAUNCH_RESUME", resume)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let pid = output.wait().parse().unwrap();
        let process = super::super::identity::tests::process(pid);
        owner.0.kill().unwrap();
        owner.0.wait().unwrap();
        stopped(&process);
    }
}
