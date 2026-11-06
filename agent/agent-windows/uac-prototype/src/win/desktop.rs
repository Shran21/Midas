use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::Duration,
};

use windows_sys::Win32::{
    Foundation::GENERIC_WRITE,
    System::{
        StationsAndDesktops::{
            CloseDesktop, DESKTOP_ACCESS_FLAGS, DESKTOP_CREATEMENU, DESKTOP_CREATEWINDOW,
            DESKTOP_ENUMERATE, DESKTOP_HOOKCONTROL, DESKTOP_READOBJECTS, DESKTOP_SWITCHDESKTOP,
            DESKTOP_WRITEOBJECTS, GetThreadDesktop, GetUserObjectInformationW, HDESK,
            OpenInputDesktop, SetThreadDesktop, UOI_NAME,
        },
        Threading::GetCurrentThreadId,
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Normal = 0,
    Secure = 1,
    Other = 2,
    /// The process may not open the input desktop, for example the secure desktop without LocalSystem.
    Inaccessible = 3,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Normal => "default",
            Kind::Secure => "secure",
            Kind::Other => "other",
            Kind::Inaccessible => "inaccessible",
        }
    }

    fn from_bits(bits: u64) -> Self {
        match bits & 3 {
            0 => Kind::Normal,
            1 => Kind::Secure,
            2 => Kind::Other,
            _ => Kind::Inaccessible,
        }
    }
}

/// Enough to read the screen and inject input; GENERIC_ALL would also ask for rights a user never holds on its own desktop.
const ACCESS: DESKTOP_ACCESS_FLAGS = DESKTOP_READOBJECTS
    | DESKTOP_WRITEOBJECTS
    | DESKTOP_CREATEWINDOW
    | DESKTOP_CREATEMENU
    | DESKTOP_ENUMERATE
    | DESKTOP_HOOKCONTROL
    | DESKTOP_SWITCHDESKTOP
    | GENERIC_WRITE;

struct Input(HDESK);

impl Input {
    fn open() -> Option<Self> {
        // SAFETY: no pointers are passed; a null handle means the desktop is not accessible.
        let handle = unsafe { OpenInputDesktop(0, 0, ACCESS) };
        (!handle.is_null()).then_some(Self(handle))
    }

    fn kind(&self) -> Kind {
        let mut name = [0u16; 64];
        let mut needed = 0;
        // SAFETY: the handle is open and the byte length matches the buffer.
        let ok = unsafe {
            GetUserObjectInformationW(
                self.0,
                UOI_NAME,
                name.as_mut_ptr().cast(),
                size_of_val(&name) as u32,
                &mut needed,
            )
        };
        if ok == 0 {
            return Kind::Other;
        }
        let length = name.iter().position(|&c| c == 0).unwrap_or(name.len());
        match String::from_utf16_lossy(&name[..length])
            .to_ascii_lowercase()
            .as_str()
        {
            "default" => Kind::Normal,
            "winlogon" => Kind::Secure,
            _ => Kind::Other,
        }
    }

    fn into_raw(self) -> HDESK {
        let handle = self.0;
        std::mem::forget(self);
        handle
    }
}

impl Drop for Input {
    fn drop(&mut self) {
        // SAFETY: the handle is open and no thread uses it.
        unsafe { CloseDesktop(self.0) };
    }
}

fn probe() -> Kind {
    Input::open().map_or(Kind::Inaccessible, |desktop| desktop.kind())
}

/// Follows the input desktop from its own thread; the generation changes on every switch.
pub struct Tracker {
    state: AtomicU64,
}

impl Tracker {
    pub fn start() -> Arc<Self> {
        let tracker = Arc::new(Self {
            state: AtomicU64::new(1 << 2 | probe() as u64),
        });
        let shared = tracker.clone();
        thread::spawn(move || {
            loop {
                thread::sleep(Duration::from_millis(50));
                let kind = probe();
                let (generation, last) = shared.current();
                if kind != last {
                    shared
                        .state
                        .store((generation + 1) << 2 | kind as u64, Ordering::Release);
                    tracing::info!(
                        desktop = kind.name(),
                        generation = generation + 1,
                        "input desktop switched"
                    );
                }
            }
        });
        tracker
    }

    pub fn current(&self) -> (u64, Kind) {
        let state = self.state.load(Ordering::Acquire);
        (state >> 2, Kind::from_bits(state))
    }
}

/// Attaches the calling thread to the input desktop; restores the thread's own desktop on drop.
pub struct Attachment {
    original: HDESK,
    current: Option<HDESK>,
}

impl Attachment {
    pub fn new() -> Self {
        // SAFETY: returns the calling thread's desktop, which must not be closed.
        let original = unsafe { GetThreadDesktop(GetCurrentThreadId()) };
        Self {
            original,
            current: None,
        }
    }

    /// The thread must not own windows or hooks, otherwise Windows refuses the switch.
    pub fn follow(&mut self) -> Kind {
        let Some(input) = Input::open() else {
            return Kind::Inaccessible;
        };
        let kind = input.kind();
        // SAFETY: the handle is open; on success it stays open while the thread uses it.
        if unsafe { SetThreadDesktop(input.0) } == 0 {
            return Kind::Inaccessible;
        }
        if let Some(previous) = self.current.replace(input.into_raw()) {
            // SAFETY: the thread no longer uses the previous handle.
            unsafe { CloseDesktop(previous) };
        }
        kind
    }
}

impl Drop for Attachment {
    fn drop(&mut self) {
        if let Some(current) = self.current.take() {
            // SAFETY: the thread returns to its own desktop before the attached handle closes.
            unsafe {
                SetThreadDesktop(self.original);
                CloseDesktop(current);
            }
        }
    }
}
