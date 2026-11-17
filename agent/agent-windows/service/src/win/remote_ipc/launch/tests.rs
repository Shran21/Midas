use std::{
    fs,
    os::windows::process::CommandExt,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use windows_sys::Win32::{
    Security::TOKEN_QUERY,
    System::{
        JobObjects::QueryInformationJobObject,
        Threading::{CreateProcessW, OpenProcessToken, SuspendThread},
    },
};

use super::*;

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
    thread::sleep(Duration::from_secs(20));
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
    let child = RunningWorker(create_child(&output, "spawn").resume().unwrap());
    let process = child.0.query_handle().unwrap();
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
fn launch_rejects_nonbroker_callers_and_ambiguous_paths() {
    let peer = super::super::identity::tests::local();
    let mut token = ptr::null_mut();
    assert_ne!(
        // SAFETY: obtains this test process's query-only token into one owned output.
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) },
        0
    );
    // SAFETY: the successful token query gave this test sole ownership of the returned handle.
    let token = unsafe { OwnedHandle::from_raw_handle(token) };
    assert!(
        SuspendedWorker::spawn(
            &peer,
            token,
            &std::env::current_exe().unwrap(),
            Worker::Privileged,
            1
        )
        .is_err()
    );
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
