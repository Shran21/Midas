use std::{
    ffi::c_void,
    io,
    os::windows::io::{AsRawHandle, OwnedHandle},
    ptr,
    sync::Mutex,
};

use tokio::sync::oneshot;
use windows_sys::Win32::{
    Foundation::WAIT_OBJECT_0,
    System::Threading::{
        CloseThreadpoolWait, CreateThreadpoolWait, DisassociateCurrentThreadFromCallback,
        PTP_CALLBACK_INSTANCE, PTP_WAIT, SetThreadpoolWait, WaitForThreadpoolWaitCallbacks,
    },
};

type State = Mutex<Option<oneshot::Sender<u32>>>;

pub(super) struct ExitWait {
    wait: PTP_WAIT,
    _state: Box<State>,
    _process: OwnedHandle,
    receiver: oneshot::Receiver<u32>,
}

impl ExitWait {
    pub(super) fn new(process: OwnedHandle) -> io::Result<Self> {
        let (sender, receiver) = oneshot::channel();
        let mut state = Box::new(Mutex::new(Some(sender)));
        // SAFETY: Box gives the callback a stable address, retained until callbacks are drained.
        let wait = unsafe {
            CreateThreadpoolWait(
                Some(completed),
                ptr::from_mut(state.as_mut()).cast(),
                ptr::null(),
            )
        };
        if wait == 0 {
            return Err(io::Error::last_os_error());
        }
        let result = Self {
            wait,
            _state: state,
            _process: process,
            receiver,
        };
        // SAFETY: the owned process has SYNCHRONIZE access and outlives this one-shot wait.
        unsafe {
            SetThreadpoolWait(result.wait, result._process.as_raw_handle(), ptr::null());
        }
        Ok(result)
    }

    pub(super) async fn wait(mut self) -> io::Result<()> {
        match (&mut self.receiver).await {
            Ok(WAIT_OBJECT_0) => Ok(()),
            _ => Err(io::Error::other("worker exit could not be observed")),
        }
    }
}

unsafe extern "system" fn completed(
    instance: PTP_CALLBACK_INSTANCE,
    context: *mut c_void,
    _: PTP_WAIT,
    result: u32,
) {
    let sender = {
        // SAFETY: the owner retains this initialized State until associated callbacks finish.
        let state = unsafe { &*context.cast::<State>() };
        state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
    };
    // SAFETY: no access to context or the wait object follows. The owned sender can wake a
    // task that drops the wait, without making that task wait for its own callback to return.
    unsafe {
        DisassociateCurrentThreadFromCallback(instance);
    }
    if let Some(sender) = sender {
        let _ = sender.send(result);
    }
}

impl Drop for ExitWait {
    fn drop(&mut self) {
        // SAFETY: disarm first, cancel queued callbacks and drain associated callbacks before
        // freeing the wait, context and retained process handle. No callback is rearmed.
        unsafe {
            SetThreadpoolWait(self.wait, ptr::null_mut(), ptr::null());
            WaitForThreadpoolWaitCallbacks(self.wait, 1);
            CloseThreadpoolWait(self.wait);
        }
    }
}
