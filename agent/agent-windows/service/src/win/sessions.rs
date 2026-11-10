use std::{
    ffi::c_void,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use midas_protocol::sessions::{MAX_SESSIONS, OsSession};
use windows_sys::Win32::System::RemoteDesktop::{
    WTS_CURRENT_SERVER_HANDLE, WTS_SESSION_INFOW, WTSEnumerateSessionsW, WTSFreeMemory,
    WTSGetActiveConsoleSessionId, WTSINFOEXW, WTSListen, WTSQuerySessionInformationW,
    WTSSessionInfoEx,
};

use crate::sessions::{NativeSession, Tracker};

#[derive(Default)]
pub struct Observer {
    epoch: AtomicU64,
    tracker: Mutex<Tracker>,
}

impl Observer {
    pub fn changed(&self) {
        self.epoch.fetch_add(1, Ordering::AcqRel);
    }

    pub fn probe(self: &Arc<Self>) -> midas_agent_core::sessions::Probe {
        let observer = self.clone();
        midas_agent_core::sessions::Probe::new(Arc::new(move || observer.collect()))
    }

    pub fn collect(&self) -> Option<Vec<OsSession>> {
        let result = self.read();
        let mut tracker = self.tracker.lock().ok()?;
        match result {
            Some((sessions, epoch)) if epoch == self.epoch.load(Ordering::Acquire) => {
                tracker.observe(sessions, epoch)
            }
            _ => {
                *tracker = Tracker::default();
                None
            }
        }
    }

    fn read(&self) -> Option<(Vec<NativeSession>, u64)> {
        let before = self.epoch.load(Ordering::Acquire);
        let first = enumerate()?;
        // SAFETY: this query has no pointer arguments or ownership transfer.
        let console = unsafe { WTSGetActiveConsoleSessionId() };
        let mut sessions = Vec::new();
        for &(id, state) in &first {
            let mut memory = Buffer::default();
            let mut length = 0;
            // SAFETY: outputs point to initialized local variables; the allocation is owned by Buffer.
            if unsafe {
                WTSQuerySessionInformationW(
                    WTS_CURRENT_SERVER_HANDLE,
                    id,
                    WTSSessionInfoEx,
                    (&mut memory.0 as *mut *mut c_void).cast(),
                    &mut length,
                )
            } == 0
                || memory.0.is_null()
                || (length as usize) < size_of::<WTSINFOEXW>()
            {
                return None;
            }
            // SAFETY: WTS returned at least one complete WTSINFOEXW in its allocation.
            let info = unsafe { &*memory.0.cast::<WTSINFOEXW>() };
            if info.Level != 1 {
                return None;
            }
            // SAFETY: Level 1 selects this union member.
            let details = unsafe { &info.Data.WTSInfoExLevel1 };
            if details.SessionId != id || details.SessionState != state {
                return None;
            }
            sessions.push(NativeSession {
                id,
                state,
                flags: details.SessionFlags,
                station: text(&details.WinStationName)?,
                user: text(&details.UserName)?,
                domain: text(&details.DomainName)?,
                logon_time: details.LogonTime,
                console: id == console,
            });
        }
        if first != enumerate()? || before != self.epoch.load(Ordering::Acquire) {
            return None;
        }
        Some((sessions, before))
    }
}

#[derive(Default)]
struct Buffer(*mut c_void);

impl Drop for Buffer {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: only allocations returned by WTS APIs are stored here, and freed once.
            unsafe { WTSFreeMemory(self.0) };
        }
    }
}

fn enumerate() -> Option<Vec<(u32, i32)>> {
    let mut memory = Buffer::default();
    let mut count = 0;
    // SAFETY: output pointers are writable; Buffer releases the returned WTS allocation.
    if unsafe {
        WTSEnumerateSessionsW(
            WTS_CURRENT_SERVER_HANDLE,
            0,
            1,
            (&mut memory.0 as *mut *mut c_void).cast(),
            &mut count,
        )
    } == 0
    {
        return None;
    }
    if count == 0 {
        return Some(Vec::new());
    }
    if memory.0.is_null() || count as usize > MAX_SESSIONS * 2 {
        return None;
    }
    // SAFETY: WTS returned count entries; the count is bounded before creating the slice.
    let entries =
        unsafe { std::slice::from_raw_parts(memory.0.cast::<WTS_SESSION_INFOW>(), count as usize) };
    let mut result: Vec<_> = entries
        .iter()
        .filter(|s| s.SessionId != 0 && s.State != WTSListen)
        .map(|s| (s.SessionId, s.State))
        .collect();
    if result.len() > MAX_SESSIONS {
        return None;
    }
    result.sort_unstable();
    Some(result)
}

fn text(value: &[u16]) -> Option<String> {
    String::from_utf16(&value[..value.iter().position(|c| *c == 0)?]).ok()
}
