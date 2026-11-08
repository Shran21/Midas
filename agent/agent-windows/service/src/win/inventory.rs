use std::{ffi::c_void, ptr};

use midas_agent_core::inventory::{Snapshot, common, insert};
use windows_sys::Win32::{
    Foundation::{ERROR_NO_MORE_ITEMS, ERROR_SUCCESS, LocalFree},
    NetworkManagement::NetManagement::{
        LOCALGROUP_MEMBERS_INFO_3, MAX_PREFERRED_LENGTH, NetApiBufferFree, NetLocalGroupGetMembers,
    },
    Security::{Authorization::ConvertStringSidToSidW, LookupAccountSidW, PSID},
    System::Registry::{
        HKEY, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY, REG_SAM_FLAGS,
        RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW,
    },
};

use super::wide;

const UNINSTALL: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
/// Built-in Administrators; the group name itself is localized.
const ADMINISTRATORS: &str = "S-1-5-32-544";

/// Common inventory plus installed software and local administrators.
pub fn collect() -> Snapshot {
    let mut snapshot = common();
    software(&mut snapshot);
    administrators(&mut snapshot);
    snapshot
}

struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: the handle came from a successful RegOpenKeyExW and is closed once.
        unsafe {
            RegCloseKey(self.0);
        }
    }
}

fn open(view: REG_SAM_FLAGS) -> Option<Key> {
    let path = wide(UNINSTALL);
    let mut key: HKEY = ptr::null_mut();
    // SAFETY: `path` is NUL-terminated and outlives the call; `key` is a valid out pointer.
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            path.as_ptr(),
            0,
            KEY_READ | view,
            &mut key,
        )
    };
    (status == ERROR_SUCCESS).then_some(Key(key))
}

fn subkeys(key: &Key) -> Vec<Vec<u16>> {
    let mut names = Vec::new();
    for index in 0.. {
        let mut name = [0u16; 256];
        let mut length = u32::try_from(name.len()).unwrap_or(0);
        // SAFETY: `name` holds `length` UTF-16 units; optional outputs are null.
        let status = unsafe {
            RegEnumKeyExW(
                key.0,
                index,
                name.as_mut_ptr(),
                &mut length,
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };
        if status == ERROR_NO_MORE_ITEMS {
            break;
        }
        if status == ERROR_SUCCESS {
            let length = usize::try_from(length).unwrap_or(0).min(name.len());
            let mut owned = name[..length].to_vec();
            owned.push(0);
            names.push(owned);
        }
    }
    names
}

fn string(key: &Key, subkey: &[u16], value: &str) -> Option<String> {
    let value = wide(value);
    let mut size = 0u32;
    // SAFETY: both names are NUL-terminated; a null buffer asks only for the size.
    let status = unsafe {
        RegGetValueW(
            key.0,
            subkey.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut size,
        )
    };
    if status != ERROR_SUCCESS || size == 0 || size > 64 * 1024 {
        return None;
    }
    let mut buffer = vec![0u16; usize::try_from(size).ok()? / 2 + 1];
    let mut size = u32::try_from(buffer.len() * 2).ok()?;
    // SAFETY: `buffer` holds `size` bytes and both names are NUL-terminated.
    let status = unsafe {
        RegGetValueW(
            key.0,
            subkey.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            ptr::null_mut(),
            buffer.as_mut_ptr().cast::<c_void>(),
            &mut size,
        )
    };
    if status != ERROR_SUCCESS {
        return None;
    }
    let end = buffer
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(buffer.len());
    Some(String::from_utf16_lossy(&buffer[..end]))
}

fn dword(key: &Key, subkey: &[u16], value: &str) -> Option<u32> {
    let value = wide(value);
    let mut data = 0u32;
    let mut size = 4u32;
    // SAFETY: `data` is a 4-byte buffer matching `size`; both names are NUL-terminated.
    let status = unsafe {
        RegGetValueW(
            key.0,
            subkey.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_DWORD,
            ptr::null_mut(),
            (&raw mut data).cast::<c_void>(),
            &mut size,
        )
    };
    (status == ERROR_SUCCESS).then_some(data)
}

fn software(snapshot: &mut Snapshot) {
    for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
        let Some(key) = open(view) else {
            continue;
        };
        for subkey in subkeys(&key) {
            if dword(&key, &subkey, "SystemComponent") == Some(1) {
                continue;
            }
            let Some(name) = string(&key, &subkey, "DisplayName") else {
                continue;
            };
            let version = string(&key, &subkey, "DisplayVersion").unwrap_or_default();
            let publisher = string(&key, &subkey, "Publisher").unwrap_or_default();
            insert(
                snapshot,
                "software",
                &name,
                format!("{version} | {publisher}"),
            );
        }
    }
}

/// Reads a NUL-terminated UTF-16 string returned by a Win32 API.
///
/// # Safety
/// `text` must be null or point to a NUL-terminated UTF-16 string.
unsafe fn from_wide(text: *const u16) -> String {
    if text.is_null() {
        return String::new();
    }
    let mut length = 0;
    // SAFETY: the caller guarantees a NUL terminator, so every read stays in bounds.
    unsafe {
        while *text.add(length) != 0 {
            length += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(text, length))
    }
}

fn administrators_group() -> Option<Vec<u16>> {
    let text = wide(ADMINISTRATORS);
    let mut sid: PSID = ptr::null_mut();
    // SAFETY: `text` is NUL-terminated; `sid` receives a LocalAlloc allocation freed below.
    if unsafe { ConvertStringSidToSidW(text.as_ptr(), &mut sid) } == 0 {
        return None;
    }
    let mut name = [0u16; 256];
    let mut name_length = u32::try_from(name.len()).unwrap_or(0);
    let mut domain = [0u16; 256];
    let mut domain_length = u32::try_from(domain.len()).unwrap_or(0);
    let mut kind = 0;
    // SAFETY: `sid` is valid; both buffers hold the lengths passed in; `sid` is freed once.
    let found = unsafe {
        let found = LookupAccountSidW(
            ptr::null(),
            sid,
            name.as_mut_ptr(),
            &mut name_length,
            domain.as_mut_ptr(),
            &mut domain_length,
            &mut kind,
        );
        LocalFree(sid);
        found
    };
    if found == 0 {
        return None;
    }
    let length = usize::try_from(name_length).ok()?.min(name.len());
    let mut group = name[..length].to_vec();
    group.push(0);
    Some(group)
}

fn administrators(snapshot: &mut Snapshot) {
    let Some(group) = administrators_group() else {
        return;
    };
    let mut buffer: *mut u8 = ptr::null_mut();
    let (mut read, mut total) = (0u32, 0u32);
    // SAFETY: `group` is NUL-terminated and all out pointers are valid for the call.
    let status = unsafe {
        NetLocalGroupGetMembers(
            ptr::null(),
            group.as_ptr(),
            3,
            &mut buffer,
            MAX_PREFERRED_LENGTH,
            &mut read,
            &mut total,
            ptr::null_mut(),
        )
    };
    if status == 0 && !buffer.is_null() {
        let count = usize::try_from(read).unwrap_or(0);
        // SAFETY: on success `buffer` points to `read` LOCALGROUP_MEMBERS_INFO_3 entries.
        let entries = unsafe {
            std::slice::from_raw_parts(buffer.cast::<LOCALGROUP_MEMBERS_INFO_3>(), count)
        };
        for entry in entries {
            // SAFETY: each entry holds a NUL-terminated name owned by `buffer`.
            let member = unsafe { from_wide(entry.lgrmi3_domainandname) };
            insert(snapshot, "admin", &member, "");
        }
    }
    if !buffer.is_null() {
        // SAFETY: `buffer` was allocated by NetLocalGroupGetMembers and is freed once.
        unsafe {
            NetApiBufferFree(buffer.cast::<c_void>());
        }
    }
}
