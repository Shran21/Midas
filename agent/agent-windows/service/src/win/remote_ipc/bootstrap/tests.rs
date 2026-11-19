use std::{
    os::windows::{io::AsHandle, process::CommandExt},
    process::{Child, Command, Stdio},
};

use windows_sys::Win32::{
    Foundation::{GetHandleInformation, WAIT_TIMEOUT},
    System::Threading::{CREATE_NO_WINDOW, CreateEventW, WaitForSingleObject},
};

use super::*;

fn still_open(handle: &OwnedHandle) {
    let mut flags = 0;
    assert_ne!(
        // SAFETY: verifies the original owned handle was not closed by a rejected import.
        unsafe { GetHandleInformation(handle.as_raw_handle(), &mut flags) },
        0
    );
}

#[test]
fn invalid_values_pseudo_handles_and_wrong_objects_are_never_adopted() {
    let image = std::env::current_exe().unwrap();
    for value in [0, u64::MAX, u64::MAX - 1, u32::MAX as u64, 0xffff_fffe, 1] {
        assert!(import_broker(value, &image).is_err());
    }
    let own = super::super::identity::tests::process(std::process::id());
    assert!(import_broker(own.as_raw_handle() as usize as u64, &image).is_err());
    still_open(&own);
    // SAFETY: creates a private non-inheritable event owned only by this test.
    let raw = unsafe { CreateEventW(ptr::null(), 1, 0, ptr::null()) };
    assert!(!raw.is_null());
    // SAFETY: successful event creation returned one owned handle.
    let event = unsafe { OwnedHandle::from_raw_handle(raw) };
    for _ in 0..16 {
        assert!(import_broker(event.as_raw_handle() as usize as u64, &image).is_err());
    }
    still_open(&event);
}

struct Fixture(Child);

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn ordinary_and_exited_processes_are_refused_without_closing_the_original() {
    let image = std::env::current_exe().unwrap();
    let mut child = Fixture(
        Command::new(&image)
            .args([
                "--exact",
                "win::remote_ipc::bootstrap::tests::child_fixture",
            ])
            .env("MIDAS_BOOTSTRAP_CHILD", "1")
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let original = child.0.as_handle().try_clone_to_owned().unwrap();
    let value = original.as_raw_handle() as usize as u64;
    assert!(import_broker(value, &image).is_err());
    still_open(&original);
    assert_eq!(
        // SAFETY: nonblocking query of this test's still-owned child handle.
        unsafe { WaitForSingleObject(original.as_raw_handle(), 0) },
        WAIT_TIMEOUT
    );
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    assert!(import_broker(value, &image).is_err());
    still_open(&original);
}

#[test]
fn child_fixture() {
    if std::env::var("MIDAS_BOOTSTRAP_CHILD").as_deref() == Ok("1") {
        std::thread::sleep(std::time::Duration::from_secs(30));
    }
}
