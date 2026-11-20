//! The Windows side of in-session transfer, in the user worker: the signed-in user's clipboard,
//! free space and folders. Everything runs with the user's own token.

use std::{io, path::Path, path::PathBuf, ptr, thread, time::Duration};

use midas_agent_core::media::{files::Platform, transfer::Problem};
use windows_sys::Win32::{
    Foundation::{GlobalFree, HWND},
    Storage::FileSystem::GetDiskFreeSpaceExW,
    System::{
        Com::CoTaskMemFree,
        DataExchange::{
            CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard,
            RegisterClipboardFormatW, SetClipboardData,
        },
        Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock},
    },
    UI::{
        Shell::{FOLDERID_Downloads, FOLDERID_Profile, KF_FLAG_DEFAULT, SHGetKnownFolderPath},
        WindowsAndMessaging::{CreateWindowExW, DestroyWindow, HWND_MESSAGE},
    },
};

use super::{from_wide, wide};

const CF_UNICODETEXT: u32 = 13;
/// Another program may hold the clipboard for a moment.
const OPEN_ATTEMPTS: u32 = 10;

/// The user's profile folder and the folder files to the device go into.
pub fn folders() -> Option<(PathBuf, PathBuf)> {
    let known = |id| {
        let mut path = ptr::null_mut();
        // SAFETY: the folder id is a valid GUID; the result is freed below whatever the outcome.
        let status =
            unsafe { SHGetKnownFolderPath(id, KF_FLAG_DEFAULT as u32, ptr::null_mut(), &mut path) };
        // SAFETY: on success `path` is a NUL-terminated string; it is read before it is freed.
        let text = (status == 0).then(|| unsafe { from_wide(path) });
        // SAFETY: SHGetKnownFolderPath allocated `path` (or left it null) for the caller to free.
        unsafe { CoTaskMemFree(path.cast()) };
        text.filter(|text| !text.is_empty()).map(PathBuf::from)
    };
    let profile = known(&FOLDERID_Profile)?;
    let downloads = known(&FOLDERID_Downloads).unwrap_or_else(|| profile.join("Downloads"));
    Some((profile, downloads.join("MIDAS")))
}

/// Owns the clipboard while text is put on it; a message-only window that receives nothing.
pub struct Windows {
    window: HWND,
}

impl Windows {
    /// On the thread that serves transfers, which keeps the value.
    pub fn new() -> io::Result<Self> {
        let class = wide("STATIC");
        // SAFETY: a system class; the window has no parent but the message-only one.
        let window = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                ptr::null(),
                0,
                0,
                0,
                0,
                0,
                HWND_MESSAGE,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null(),
            )
        };
        if window.is_null() {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { window })
    }

    fn open(&self) -> Result<Opened, Problem> {
        for _ in 0..OPEN_ATTEMPTS {
            // SAFETY: the window belongs to this thread and lives as long as `self`.
            if unsafe { OpenClipboard(self.window) } != 0 {
                return Ok(Opened);
            }
            thread::sleep(Duration::from_millis(25));
        }
        Err(Problem::Unavailable)
    }
}

impl Drop for Windows {
    fn drop(&mut self) {
        // SAFETY: destroys the window this value created on this thread.
        unsafe { DestroyWindow(self.window) };
    }
}

/// The clipboard is open until this is dropped.
struct Opened;
impl Drop for Opened {
    fn drop(&mut self) {
        // SAFETY: closes the clipboard this thread opened.
        unsafe { CloseClipboard() };
    }
}

/// Moveable global memory with `bytes` copied in, for SetClipboardData.
fn global(bytes: &[u8]) -> Option<*mut core::ffi::c_void> {
    // SAFETY: a plain allocation; freed below unless the clipboard takes it.
    let memory = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes.len()) };
    if memory.is_null() {
        return None;
    }
    // SAFETY: the block is at least `bytes.len()` long and locked while written.
    unsafe {
        let target = GlobalLock(memory);
        if target.is_null() {
            GlobalFree(memory);
            return None;
        }
        ptr::copy_nonoverlapping(bytes.as_ptr(), target.cast(), bytes.len());
        GlobalUnlock(memory);
    }
    Some(memory)
}

/// Puts `memory` on the open clipboard, which then owns it.
fn set(format: u32, memory: *mut core::ffi::c_void) -> bool {
    // SAFETY: the clipboard is open and owned by this thread's window; ownership passes on success.
    let taken = !unsafe { SetClipboardData(format, memory) }.is_null();
    if !taken {
        // SAFETY: the clipboard did not take the block, so it is still ours.
        unsafe { GlobalFree(memory) };
    }
    taken
}

impl Platform for Windows {
    fn clipboard(&mut self) -> Result<Option<String>, Problem> {
        let _open = self.open()?;
        // SAFETY: the clipboard is open; the handle is only used while it stays open.
        let data = unsafe { GetClipboardData(CF_UNICODETEXT) };
        if data.is_null() {
            return Ok(None);
        }
        // SAFETY: CF_UNICODETEXT data is global memory of UTF-16 units; reads stay within its size.
        unsafe {
            let units = GlobalSize(data) / 2;
            let text = GlobalLock(data).cast::<u16>();
            if text.is_null() {
                return Err(Problem::Unavailable);
            }
            let slice = std::slice::from_raw_parts(text, units);
            let length = slice.iter().position(|&c| c == 0).unwrap_or(units);
            let value = String::from_utf16_lossy(&slice[..length]);
            GlobalUnlock(data);
            Ok(Some(value))
        }
    }

    fn set_clipboard(&mut self, text: &str) -> Result<(), Problem> {
        let units: Vec<u8> = text
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        let _open = self.open()?;
        // SAFETY: the clipboard is open and owned by this thread's window.
        if unsafe { EmptyClipboard() } == 0 {
            return Err(Problem::Unavailable);
        }
        let memory = global(&units).ok_or(Problem::Unavailable)?;
        if !set(CF_UNICODETEXT, memory) {
            return Err(Problem::Unavailable);
        }
        // The technician's text stays out of the user's clipboard history and cloud clipboard.
        for name in ["CanIncludeInClipboardHistory", "CanUploadToCloudClipboard"] {
            let name = wide(name);
            // SAFETY: a NUL-terminated name.
            let format = unsafe { RegisterClipboardFormatW(name.as_ptr()) };
            if format != 0
                && let Some(memory) = global(&0u32.to_le_bytes())
            {
                set(format, memory);
            }
        }
        Ok(())
    }

    fn free_space(&self, path: &Path) -> Option<u64> {
        let path = wide(path.to_str()?);
        let mut free = 0u64;
        // SAFETY: a NUL-terminated path and one valid output; the other outputs are optional.
        let ok = unsafe {
            GetDiskFreeSpaceExW(path.as_ptr(), &mut free, ptr::null_mut(), ptr::null_mut())
        };
        (ok != 0).then_some(free)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_users_folders_and_free_space_are_found() {
        let (profile, folder) = folders().expect("a profile folder");
        assert!(profile.is_absolute());
        assert!(folder.ends_with("MIDAS"));
        let Ok(windows) = Windows::new() else {
            return;
        };
        assert!(windows.free_space(&profile).is_some_and(|free| free > 0));
    }
}
