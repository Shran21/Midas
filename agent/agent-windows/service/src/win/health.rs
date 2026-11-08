use std::ptr;

use midas_agent_core::health::{
    ANTIVIRUS, ENCRYPTION, Finding, SERVICES, STORAGE, UPDATES, combine, days_between, result,
    services as services_check, space, unavailable, volumes,
};
use midas_protocol::{HealthCheck, HealthState};
use windows::{
    Win32::{
        Foundation::E_ACCESSDENIED,
        System::{
            Com::{
                CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
                CoSetProxyBlanket, CoUninitialize, EOAC_NONE, RPC_C_AUTHN_LEVEL_CALL,
                RPC_C_IMP_LEVEL_IMPERSONATE,
            },
            UpdateAgent::{AutomaticUpdates, IAutomaticUpdates2},
            Variant::{VARIANT, VT_DATE, VT_EMPTY, VT_NULL},
            Wmi::{
                IWbemClassObject, IWbemLocator, WBEM_E_ACCESS_DENIED, WBEM_E_INVALID_CLASS,
                WBEM_E_INVALID_NAMESPACE, WBEM_FLAG_CONNECT_USE_MAX_WAIT, WBEM_FLAG_FORWARD_ONLY,
                WBEM_FLAG_RETURN_IMMEDIATELY, WbemLocator,
            },
        },
    },
    core::{BSTR, HRESULT, PCWSTR},
};
use windows_sys::Win32::{
    Foundation::{ERROR_MORE_DATA, ERROR_SUCCESS, GetLastError},
    System::{
        Registry::{
            HKEY, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY, RegCloseKey, RegOpenKeyExW,
        },
        SecurityCenter::{
            WSC_SECURITY_PROVIDER_ANTIVIRUS, WSC_SECURITY_PROVIDER_HEALTH_GOOD,
            WscGetSecurityProviderHealth,
        },
        Services::{
            CloseServiceHandle, ENUM_SERVICE_STATUS_PROCESSW, EnumServicesStatusExW,
            OpenSCManagerW, OpenServiceW, QUERY_SERVICE_CONFIGW, QueryServiceConfig2W,
            QueryServiceConfigW, SC_ENUM_PROCESS_INFO, SC_HANDLE, SC_MANAGER_CONNECT,
            SC_MANAGER_ENUMERATE_SERVICE, SERVICE_AUTO_START, SERVICE_CONFIG_TRIGGER_INFO,
            SERVICE_INACTIVE, SERVICE_QUERY_CONFIG, SERVICE_TRIGGER_INFO, SERVICE_WIN32,
        },
    },
};

use super::{from_wide, wide};

pub const CHECKS: [&str; 5] = [SERVICES, UPDATES, ANTIVIRUS, ENCRYPTION, STORAGE];

const RPC_C_AUTHN_WINNT: u32 = 10;
const RPC_C_AUTHZ_NONE: u32 = 0;
/// Milliseconds to wait for each WMI row; a stuck provider must not hold the checks.
const ROW_TIMEOUT: i32 = 30_000;
/// A service stopped with one of these did so on purpose or never ran.
const ERROR_SERVICE_NEVER_STARTED: u32 = 1077;
const REBOOT_KEYS: [&str; 2] = [
    r"SOFTWARE\Microsoft\Windows\CurrentVersion\WindowsUpdate\Auto Update\RebootRequired",
    r"SOFTWARE\Microsoft\Windows\CurrentVersion\Component Based Servicing\RebootPending",
];

pub fn check() -> Vec<HealthCheck> {
    let com = Com::init();
    let com = com.is_some();
    vec![
        services(),
        updates(com),
        antivirus(com),
        encryption(com),
        storage(com),
    ]
}

struct Com;

impl Com {
    fn init() -> Option<Self> {
        // SAFETY: initialises COM for this blocking thread; Drop balances a successful call.
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
            .is_ok()
            .then_some(Self)
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        // SAFETY: matches the successful CoInitializeEx on this thread.
        unsafe { CoUninitialize() }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Failure {
    Missing,
    Denied,
    Error,
}

fn failure(error: &windows::core::Error) -> Failure {
    let code = error.code();
    if code == HRESULT(WBEM_E_INVALID_NAMESPACE.0) || code == HRESULT(WBEM_E_INVALID_CLASS.0) {
        Failure::Missing
    } else if code == HRESULT(WBEM_E_ACCESS_DENIED.0) || code == E_ACCESSDENIED {
        Failure::Denied
    } else {
        Failure::Error
    }
}

fn not_available(id: &str, subject: &str, failure: &Failure) -> HealthCheck {
    match failure {
        Failure::Missing => unavailable(
            id,
            "not_supported",
            &format!("{subject} is not available on this system"),
        ),
        Failure::Denied => unavailable(
            id,
            "access_denied",
            &format!("{subject} needs the agent service rights"),
        ),
        Failure::Error => unavailable(id, "error", &format!("{subject} could not be queried")),
    }
}

struct Row(IWbemClassObject);

impl Row {
    fn get(&self, name: &str) -> Option<VARIANT> {
        let name = wide(name);
        let mut value = VARIANT::default();
        // SAFETY: `name` is NUL-terminated and outlives the call; `value` is a valid out pointer.
        unsafe {
            self.0
                .Get(PCWSTR(name.as_ptr()), 0, &mut value, None, None)
                .ok()?;
        }
        (!matches!(value.vt(), VT_EMPTY | VT_NULL)).then_some(value)
    }

    fn flag(&self, name: &str) -> Option<bool> {
        self.get(name).and_then(|value| bool::try_from(&value).ok())
    }

    fn number(&self, name: &str) -> Option<u32> {
        self.get(name).and_then(|value| u32::try_from(&value).ok())
    }

    fn text(&self, name: &str) -> Option<String> {
        self.get(name)
            .and_then(|value| BSTR::try_from(&value).ok())
            .map(|text| text.to_string())
    }
}

/// Runs a WQL query on the local machine. COM must be initialised on this thread.
fn query(namespace: &str, wql: &str) -> Result<Vec<Row>, Failure> {
    let run = || -> windows::core::Result<Vec<Row>> {
        // SAFETY: COM is initialised on this thread; every BSTR outlives the call that uses it
        // and the returned interfaces are reference counted.
        unsafe {
            let locator: IWbemLocator = CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER)?;
            let services = locator.ConnectServer(
                &BSTR::from(namespace),
                &BSTR::new(),
                &BSTR::new(),
                &BSTR::new(),
                WBEM_FLAG_CONNECT_USE_MAX_WAIT.0,
                &BSTR::new(),
                None,
            )?;
            CoSetProxyBlanket(
                &services,
                RPC_C_AUTHN_WINNT,
                RPC_C_AUTHZ_NONE,
                PCWSTR::null(),
                RPC_C_AUTHN_LEVEL_CALL,
                RPC_C_IMP_LEVEL_IMPERSONATE,
                None,
                EOAC_NONE,
            )?;
            let rows = services.ExecQuery(
                &BSTR::from("WQL"),
                &BSTR::from(wql),
                WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY,
                None,
            )?;
            let mut found = Vec::new();
            loop {
                let mut row = [None];
                let mut returned = 0;
                rows.Next(ROW_TIMEOUT, &mut row, &mut returned).ok()?;
                match row[0].take() {
                    Some(row) if returned == 1 => found.push(Row(row)),
                    _ => break,
                }
            }
            Ok(found)
        }
    };
    run().map_err(|error| failure(&error))
}

struct Service(SC_HANDLE);

impl Drop for Service {
    fn drop(&mut self) {
        // SAFETY: the handle came from a successful Open* call and is closed once.
        unsafe {
            CloseServiceHandle(self.0);
        }
    }
}

/// Reads a variable-size service structure into 8-byte aligned storage.
fn service_buffer(fill: impl Fn(*mut u8, u32, &mut u32) -> i32) -> Option<Vec<u64>> {
    let mut needed = 0u32;
    if fill(ptr::null_mut(), 0, &mut needed) != 0 || needed == 0 || needed > 64 * 1024 {
        return None;
    }
    let mut buffer = vec![0u64; usize::try_from(needed).ok()?.div_ceil(8)];
    let size = u32::try_from(buffer.len() * 8).ok()?;
    (fill(buffer.as_mut_ptr().cast(), size, &mut needed) != 0).then_some(buffer)
}

/// Automatic start without triggers: the service is meant to run all the time.
fn always_on(manager: &Service, name: *const u16) -> bool {
    // SAFETY: `name` is a NUL-terminated name owned by the enumeration buffer.
    let handle = unsafe { OpenServiceW(manager.0, name, SERVICE_QUERY_CONFIG) };
    if handle.is_null() {
        return false;
    }
    let service = Service(handle);
    let Some(config) = service_buffer(|buffer, size, needed| {
        // SAFETY: `buffer` holds `size` bytes (or is null with size 0) and `needed` is valid.
        unsafe { QueryServiceConfigW(service.0, buffer.cast(), size, needed) }
    }) else {
        return false;
    };
    // SAFETY: on success the buffer starts with an aligned QUERY_SERVICE_CONFIGW.
    let start = unsafe { (*config.as_ptr().cast::<QUERY_SERVICE_CONFIGW>()).dwStartType };
    if start != SERVICE_AUTO_START {
        return false;
    }
    service_buffer(|buffer, size, needed| {
        // SAFETY: as above, for the trigger information level.
        unsafe {
            QueryServiceConfig2W(service.0, SERVICE_CONFIG_TRIGGER_INFO, buffer, size, needed)
        }
    })
    // SAFETY: on success the buffer starts with an aligned SERVICE_TRIGGER_INFO.
    .is_none_or(|info| unsafe { (*info.as_ptr().cast::<SERVICE_TRIGGER_INFO>()).cTriggers } == 0)
}

/// Stopped automatic services whose last exit reported an error.
fn stopped_services() -> Option<Vec<String>> {
    // SAFETY: null names select the local service control manager.
    let handle = unsafe {
        OpenSCManagerW(
            ptr::null(),
            ptr::null(),
            SC_MANAGER_CONNECT | SC_MANAGER_ENUMERATE_SERVICE,
        )
    };
    if handle.is_null() {
        return None;
    }
    let manager = Service(handle);
    let mut buffer = vec![0u64; 8 * 1024];
    let mut resume = 0u32;
    let mut failed = Vec::new();
    for _ in 0..64 {
        let (mut needed, mut returned) = (0u32, 0u32);
        let size = u32::try_from(buffer.len() * 8).ok()?;
        // SAFETY: `buffer` holds `size` bytes; every out pointer is valid for the call.
        let ok = unsafe {
            EnumServicesStatusExW(
                manager.0,
                SC_ENUM_PROCESS_INFO,
                SERVICE_WIN32,
                SERVICE_INACTIVE,
                buffer.as_mut_ptr().cast(),
                size,
                &mut needed,
                &mut returned,
                &mut resume,
                ptr::null(),
            )
        };
        // SAFETY: reads the error of the call above on this thread.
        let more = ok == 0 && unsafe { GetLastError() } == ERROR_MORE_DATA;
        if ok == 0 && !more {
            return None;
        }
        let count = usize::try_from(returned).ok()?;
        // SAFETY: the call wrote `returned` aligned entries at the start of `buffer`.
        let entries = unsafe {
            std::slice::from_raw_parts(
                buffer.as_ptr().cast::<ENUM_SERVICE_STATUS_PROCESSW>(),
                count,
            )
        };
        for entry in entries {
            let exit = entry.ServiceStatusProcess.dwWin32ExitCode;
            if exit == ERROR_SUCCESS || exit == ERROR_SERVICE_NEVER_STARTED {
                continue;
            }
            if always_on(&manager, entry.lpServiceName) {
                // SAFETY: both names are NUL-terminated strings inside `buffer`.
                let (name, display) = unsafe {
                    (
                        from_wide(entry.lpServiceName),
                        from_wide(entry.lpDisplayName),
                    )
                };
                failed.push(format!("{display} ({name})"));
            }
        }
        if !more {
            return Some(failed);
        }
        let wanted = usize::try_from(needed).ok()?.div_ceil(8);
        if wanted > buffer.len() {
            buffer.resize(wanted.min(64 * 1024), 0);
        }
    }
    Some(failed)
}

fn services() -> HealthCheck {
    match stopped_services() {
        Some(failed) => services_check(&failed),
        None => unavailable(SERVICES, "error", "The service list could not be read"),
    }
}

fn key_exists(path: &str) -> bool {
    let path = wide(path);
    let mut key: HKEY = ptr::null_mut();
    // SAFETY: `path` is NUL-terminated; `key` is a valid out pointer and closed below.
    unsafe {
        if RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            path.as_ptr(),
            0,
            KEY_READ | KEY_WOW64_64KEY,
            &mut key,
        ) != ERROR_SUCCESS
        {
            return false;
        }
        RegCloseKey(key);
    }
    true
}

/// OLE automation date (days since 1899-12-30) to Unix seconds.
fn unix_from_ole(value: &VARIANT) -> Option<i64> {
    if value.vt() != VT_DATE {
        return None;
    }
    // SAFETY: the type tag is VT_DATE, so the union holds the `date` member.
    let days = unsafe { value.Anonymous.Anonymous.Anonymous.date };
    let seconds = (days - 25_569.0) * 86_400.0;
    (seconds.is_finite() && seconds > 0.0 && seconds < 1e12).then_some(seconds as i64)
}

/// Last successful installation and search, from the Windows Update agent.
fn update_dates() -> Option<(Option<i64>, Option<i64>)> {
    // SAFETY: COM is initialised on this thread; the interfaces are reference counted.
    unsafe {
        let updates: IAutomaticUpdates2 =
            CoCreateInstance(&AutomaticUpdates, None, CLSCTX_INPROC_SERVER).ok()?;
        let results = updates.Results().ok()?;
        let installed = results
            .LastInstallationSuccessDate()
            .ok()
            .and_then(|date| unix_from_ole(&date));
        let searched = results
            .LastSearchSuccessDate()
            .ok()
            .and_then(|date| unix_from_ole(&date));
        Some((installed, searched))
    }
}

fn update_findings(
    reboot: bool,
    dates: Option<(Option<i64>, Option<i64>)>,
    now: i64,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    if reboot {
        findings.push(Finding::new(
            HealthState::Warning,
            "reboot_pending",
            "A restart is pending to finish updates",
            [],
        ));
    }
    let Some((installed, searched)) = dates else {
        return findings;
    };
    match installed.map(|at| days_between(at, now)) {
        None => findings.push(Finding::new(
            HealthState::Warning,
            "outdated",
            "No successful update installation is recorded",
            [],
        )),
        Some(days) if days > 35 => findings.push(Finding::new(
            if days > 60 {
                HealthState::Critical
            } else {
                HealthState::Warning
            },
            "outdated",
            format!("Last successful update installation {days} days ago"),
            [days.to_string()],
        )),
        Some(_) => {}
    }
    if let Some(days) = searched.map(|at| days_between(at, now))
        && days > 7
    {
        findings.push(Finding::new(
            HealthState::Warning,
            "search_failed",
            format!("Last successful update search {days} days ago"),
            [days.to_string()],
        ));
    }
    findings
}

fn updates(com: bool) -> HealthCheck {
    let reboot = REBOOT_KEYS.iter().any(|key| key_exists(key));
    let dates = if com { update_dates() } else { None };
    if dates.is_none() && !reboot {
        return unavailable(UPDATES, "error", "Windows Update status could not be read");
    }
    let findings = update_findings(reboot, dates, midas_agent_core::inventory::unix_now());
    combine(UPDATES, "current", "Updates are current", &findings)
}

#[derive(Debug, Default)]
struct Defender {
    service: Option<bool>,
    antivirus: Option<bool>,
    realtime: Option<bool>,
    signature_days: Option<u32>,
    mode: Option<String>,
    /// Security Center reports another antivirus product as healthy.
    other_product: bool,
}

fn defender(status: &Defender) -> HealthCheck {
    let off = status.service == Some(false)
        || status.antivirus == Some(false)
        || status.realtime == Some(false);
    if off && status.other_product {
        return result(
            ANTIVIRUS,
            HealthState::Ok,
            "other_product",
            "Another antivirus product protects this device",
            &[],
        );
    }
    let passive = status
        .mode
        .as_deref()
        .is_some_and(|mode| mode.contains("Passive") || mode.contains("EDR"));
    if passive {
        return result(
            ANTIVIRUS,
            HealthState::Ok,
            "passive",
            "Microsoft Defender runs in passive mode next to another antivirus product",
            &[],
        );
    }
    let mut findings = Vec::new();
    if status.service == Some(false) || status.antivirus == Some(false) {
        findings.push(Finding::new(
            HealthState::Critical,
            "disabled",
            "Microsoft Defender Antivirus is turned off",
            [],
        ));
    } else if status.realtime == Some(false) {
        findings.push(Finding::new(
            HealthState::Critical,
            "realtime_off",
            "Real-time protection is turned off",
            [],
        ));
    }
    if let Some(days) = status.signature_days
        && days > 3
    {
        findings.push(Finding::new(
            if days > 7 {
                HealthState::Critical
            } else {
                HealthState::Warning
            },
            "signatures_old",
            format!("Security intelligence is {days} days old"),
            [days.to_string()],
        ));
    }
    combine(
        ANTIVIRUS,
        "protected",
        "Microsoft Defender protection is on",
        &findings,
    )
}

fn antivirus(com: bool) -> HealthCheck {
    if !com {
        return not_available(ANTIVIRUS, "Microsoft Defender", &Failure::Error);
    }
    let rows = match query(
        r"ROOT\Microsoft\Windows\Defender",
        "SELECT AMServiceEnabled, AntivirusEnabled, RealTimeProtectionEnabled, AntivirusSignatureAge, AMRunningMode FROM MSFT_MpComputerStatus",
    ) {
        Ok(rows) => rows,
        Err(failure) => return not_available(ANTIVIRUS, "Microsoft Defender", &failure),
    };
    let Some(row) = rows.first() else {
        return not_available(ANTIVIRUS, "Microsoft Defender", &Failure::Missing);
    };
    defender(&Defender {
        service: row.flag("AMServiceEnabled"),
        antivirus: row.flag("AntivirusEnabled"),
        realtime: row.flag("RealTimeProtectionEnabled"),
        signature_days: row.number("AntivirusSignatureAge"),
        mode: row.text("AMRunningMode"),
        other_product: security_center_protected(),
    })
}

/// Security Center exists on client editions only; elsewhere this is false.
fn security_center_protected() -> bool {
    let mut health = 0;
    // SAFETY: `health` is a valid out pointer for the duration of the call.
    let status = unsafe {
        WscGetSecurityProviderHealth(WSC_SECURITY_PROVIDER_ANTIVIRUS as u32, &mut health)
    };
    status == 0 && health == WSC_SECURITY_PROVIDER_HEALTH_GOOD
}

#[derive(Debug)]
struct Encryptable {
    letter: String,
    kind: Option<u32>,
    protection: Option<u32>,
    conversion: Option<u32>,
}

fn bitlocker(volumes: &[Encryptable]) -> HealthCheck {
    let fixed: Vec<&Encryptable> = volumes
        .iter()
        .filter(|volume| !volume.letter.is_empty() && matches!(volume.kind, None | Some(0 | 1)))
        .collect();
    if fixed.is_empty() {
        return unavailable(
            ENCRYPTION,
            "not_supported",
            "BitLocker reports no fixed volume",
        );
    }
    let mut changing = Vec::new();
    let mut suspended = Vec::new();
    let mut open = Vec::new();
    for volume in fixed {
        match (volume.conversion, volume.protection) {
            (Some(2..=5), _) => changing.push(volume.letter.clone()),
            (_, Some(1)) => {}
            (Some(1), _) => suspended.push(volume.letter.clone()),
            _ => open.push(volume.letter.clone()),
        }
    }
    let mut findings = Vec::new();
    for (letters, code, summary) in [
        (
            open,
            "not_encrypted",
            "volume(s) not protected by BitLocker",
        ),
        (
            suspended,
            "suspended",
            "volume(s) with BitLocker protection suspended",
        ),
        (
            changing,
            "in_progress",
            "volume(s) being encrypted or decrypted",
        ),
    ] {
        if !letters.is_empty() {
            findings.push(Finding::new(
                HealthState::Warning,
                code,
                format!("{} {summary}", letters.len()),
                letters,
            ));
        }
    }
    combine(
        ENCRYPTION,
        "encrypted",
        "All fixed volumes are protected by BitLocker",
        &findings,
    )
}

fn encryption(com: bool) -> HealthCheck {
    if !com {
        return not_available(ENCRYPTION, "BitLocker", &Failure::Error);
    }
    match query(
        r"ROOT\CIMV2\Security\MicrosoftVolumeEncryption",
        "SELECT DriveLetter, VolumeType, ProtectionStatus, ConversionStatus FROM Win32_EncryptableVolume",
    ) {
        Ok(rows) => bitlocker(
            &rows
                .iter()
                .map(|row| Encryptable {
                    letter: row.text("DriveLetter").unwrap_or_default(),
                    kind: row.number("VolumeType"),
                    protection: row.number("ProtectionStatus"),
                    conversion: row.number("ConversionStatus"),
                })
                .collect::<Vec<_>>(),
        ),
        Err(failure) => not_available(ENCRYPTION, "BitLocker", &failure),
    }
}

/// Physical disks by health: 1 is a warning, 2 unhealthy (MSFT_PhysicalDisk.HealthStatus).
fn disk_findings(disks: &[(String, u32)]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (status, state, code, summary) in [
        (
            2,
            HealthState::Critical,
            "disk_unhealthy",
            "disk(s) report a failure",
        ),
        (
            1,
            HealthState::Warning,
            "disk_warning",
            "disk(s) report a warning",
        ),
    ] {
        let names: Vec<String> = disks
            .iter()
            .filter(|(_, health)| *health == status)
            .map(|(name, _)| name.clone())
            .collect();
        if !names.is_empty() {
            findings.push(Finding::new(
                state,
                code,
                format!("{} {summary}", names.len()),
                names,
            ));
        }
    }
    findings
}

fn storage(com: bool) -> HealthCheck {
    let volumes = volumes();
    let disks = com
        .then(|| {
            query(
                r"ROOT\Microsoft\Windows\Storage",
                "SELECT FriendlyName, HealthStatus FROM MSFT_PhysicalDisk",
            )
            .ok()
        })
        .flatten();
    if volumes.is_empty() && disks.is_none() {
        return unavailable(STORAGE, "error", "Volumes and disks could not be read");
    }
    let mut findings = space(&volumes);
    if let Some(rows) = disks {
        let disks: Vec<(String, u32)> = rows
            .iter()
            .filter_map(|row| {
                Some((
                    row.text("FriendlyName").unwrap_or_default(),
                    row.number("HealthStatus")?,
                ))
            })
            .collect();
        findings.extend(disk_findings(&disks));
    }
    combine(
        STORAGE,
        "healthy",
        "Disks and free space are healthy",
        &findings,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(check: &HealthCheck) -> HealthState {
        HealthState::try_from(check.state).unwrap()
    }

    #[test]
    fn defender_failures_are_reported() {
        let healthy = Defender {
            service: Some(true),
            antivirus: Some(true),
            realtime: Some(true),
            signature_days: Some(0),
            mode: Some("Normal".into()),
            other_product: false,
        };
        assert_eq!(defender(&healthy).code, "protected");
        let off = defender(&Defender {
            antivirus: Some(false),
            ..Defender::default()
        });
        assert_eq!(
            (state(&off), off.code.as_str()),
            (HealthState::Critical, "disabled")
        );
        let realtime = defender(&Defender {
            realtime: Some(false),
            signature_days: Some(5),
            ..Defender::default()
        });
        assert_eq!(
            (state(&realtime), realtime.code.as_str()),
            (HealthState::Critical, "realtime_off")
        );
        assert!(realtime.summary.contains("5 days old"));
        let stale = defender(&Defender {
            signature_days: Some(9),
            ..Defender::default()
        });
        assert_eq!(
            (
                state(&stale),
                stale.code.as_str(),
                stale.subjects.as_slice()
            ),
            (
                HealthState::Critical,
                "signatures_old",
                ["9".to_owned()].as_slice()
            )
        );
        let passive = defender(&Defender {
            realtime: Some(false),
            mode: Some("Passive Mode".into()),
            ..Defender::default()
        });
        assert_eq!(
            (state(&passive), passive.code.as_str()),
            (HealthState::Ok, "passive")
        );
        let replaced = defender(&Defender {
            antivirus: Some(false),
            signature_days: Some(400),
            other_product: true,
            ..Defender::default()
        });
        assert_eq!(
            (state(&replaced), replaced.code.as_str()),
            (HealthState::Ok, "other_product")
        );
    }

    #[test]
    fn bitlocker_states_are_reported_per_volume() {
        let volume = |letter: &str, kind, protection, conversion| Encryptable {
            letter: letter.into(),
            kind: Some(kind),
            protection: Some(protection),
            conversion: Some(conversion),
        };
        let all = bitlocker(&[volume("C:", 0, 1, 1), volume("", 1, 0, 0)]);
        assert_eq!(
            (state(&all), all.code.as_str()),
            (HealthState::Ok, "encrypted")
        );
        let open = bitlocker(&[
            volume("C:", 0, 0, 0),
            volume("D:", 1, 1, 1),
            volume("E:", 2, 0, 0),
        ]);
        assert_eq!(
            (state(&open), open.code.as_str(), open.subjects.as_slice()),
            (
                HealthState::Warning,
                "not_encrypted",
                ["C:".to_owned()].as_slice()
            )
        );
        let suspended = bitlocker(&[volume("C:", 0, 0, 1), volume("D:", 1, 0, 2)]);
        assert_eq!(suspended.code, "suspended");
        assert!(suspended.summary.contains("being encrypted"));
        let none = bitlocker(&[volume("E:", 2, 0, 0)]);
        assert_eq!(
            (state(&none), none.code.as_str()),
            (HealthState::Unavailable, "not_supported")
        );
    }

    #[test]
    fn update_age_and_pending_restart_are_reported() {
        let now = 1_800_000_000;
        let day = 86_400;
        assert!(update_findings(false, Some((Some(now - day), Some(now))), now).is_empty());
        let old = update_findings(true, Some((Some(now - 40 * day), Some(now - 9 * day))), now);
        let codes: Vec<_> = old
            .iter()
            .map(|finding| (finding.state, finding.code))
            .collect();
        assert_eq!(
            codes,
            [
                (HealthState::Warning, "reboot_pending"),
                (HealthState::Warning, "outdated"),
                (HealthState::Warning, "search_failed"),
            ]
        );
        let ancient = update_findings(false, Some((Some(now - 61 * day), None)), now);
        assert_eq!(
            (ancient[0].state, ancient[0].code),
            (HealthState::Critical, "outdated")
        );
        let never = update_findings(false, Some((None, Some(now))), now);
        assert_eq!(never[0].code, "outdated");
        assert_eq!(update_findings(true, None, now).len(), 1);
    }

    #[test]
    fn unhealthy_disks_are_critical_and_warnings_are_warnings() {
        let findings = disk_findings(&[("SSD".into(), 0), ("HDD".into(), 2), ("NVMe".into(), 1)]);
        assert_eq!(findings.len(), 2);
        assert_eq!(
            (findings[0].state, findings[0].subjects.as_slice()),
            (HealthState::Critical, ["HDD".to_owned()].as_slice())
        );
        assert_eq!(findings[1].code, "disk_warning");
    }

    #[test]
    fn wmi_failures_map_to_unavailable_reasons() {
        let missing = windows::core::Error::from(HRESULT(WBEM_E_INVALID_NAMESPACE.0));
        assert_eq!(failure(&missing), Failure::Missing);
        assert_eq!(
            failure(&windows::core::Error::from(E_ACCESSDENIED)),
            Failure::Denied
        );
        let check = not_available(ENCRYPTION, "BitLocker", &Failure::Denied);
        assert_eq!(
            (state(&check), check.code.as_str()),
            (HealthState::Unavailable, "access_denied")
        );
    }
}
