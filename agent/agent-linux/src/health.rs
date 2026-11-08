use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant, SystemTime},
};

use midas_agent_core::health::{
    ANTIVIRUS, ENCRYPTION, Finding, SERVICES, STORAGE, Thresholds, UPDATES, combine, result,
    services, space, unavailable, volumes,
};
use midas_protocol::{HealthCheck, HealthState};

/// Antivirus has no Linux check yet; it is reported as unavailable, not left out.
pub const CHECKS: [&str; 4] = [SERVICES, UPDATES, ENCRYPTION, STORAGE];

const COMMAND_TIMEOUT: Duration = Duration::from_secs(60);
const REBOOT_REQUIRED: &str = "/run/reboot-required";
const APT_LISTS: &str = "/var/lib/apt/lists";
const MOUNTINFO: &str = "/proc/self/mountinfo";
const BLOCK_DEVICES: &str = "/sys/dev/block";
const EXT4: &str = "/sys/fs/ext4";

pub fn check(limits: &Thresholds) -> Vec<HealthCheck> {
    vec![
        service_check(),
        updates(),
        unavailable(
            ANTIVIRUS,
            "not_supported",
            "No antivirus check is available on Linux",
        ),
        encryption(),
        storage(limits),
    ]
}

/// Runs a command with a time limit and the C locale; `None` if it cannot start or hangs.
fn run(program: &str, args: &[&str]) -> Option<(i32, String)> {
    let mut child = Command::new(program)
        .args(args)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut text = Vec::new();
        let _ = stdout.by_ref().take(4 * 1024 * 1024).read_to_end(&mut text);
        String::from_utf8_lossy(&text).into_owned()
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < COMMAND_TIMEOUT => {
                std::thread::sleep(Duration::from_millis(100));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    let output = reader.join().ok()?;
    Some((status.code().unwrap_or(-1), output))
}

/// Unit names from `systemctl list-units --plain --no-legend`.
pub fn failed_units(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .filter(|unit| unit.contains('.'))
        .map(str::to_owned)
        .collect()
}

fn service_check() -> HealthCheck {
    match run(
        "systemctl",
        &[
            "list-units",
            "--type=service",
            "--state=failed",
            "--plain",
            "--no-legend",
            "--no-pager",
        ],
    ) {
        Some((0, output)) => services(&failed_units(&output)),
        _ => unavailable(SERVICES, "error", "systemd units could not be listed"),
    }
}

/// Pending upgrades from `apt-get -s upgrade`: (all, from a security archive).
pub fn apt_pending(output: &str) -> (usize, usize) {
    let lines: Vec<&str> = output
        .lines()
        .filter(|line| line.starts_with("Inst "))
        .collect();
    let security = lines
        .iter()
        .filter(|line| line.contains("-security") || line.contains("Debian-Security"))
        .count();
    (lines.len(), security)
}

/// Package lines of `dnf check-update` or `dnf updateinfo list`, without headings.
pub fn dnf_lines(output: &str) -> usize {
    output
        .lines()
        .filter(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            fields.len() >= 3 && !line.starts_with(' ') && !line.contains("metadata")
        })
        .count()
}

fn pending_updates() -> Option<(usize, usize)> {
    if Path::new("/usr/bin/apt-get").exists() {
        let (code, output) = run(
            "apt-get",
            &["-s", "-q", "-o", "Debug::NoLocking=1", "upgrade"],
        )?;
        return (code == 0).then(|| apt_pending(&output));
    }
    let (code, output) = run("dnf", &["-q", "--cacheonly", "check-update"])?;
    let total = match code {
        0 => 0,
        100 => dnf_lines(&output),
        _ => return None,
    };
    let security = match run(
        "dnf",
        &["-q", "--cacheonly", "updateinfo", "list", "--security"],
    ) {
        Some((0, output)) => dnf_lines(&output),
        _ => 0,
    };
    Some((total, security))
}

/// Days since the package lists were last refreshed, from the newest list file.
fn lists_age(directory: &Path, now: SystemTime) -> Option<i64> {
    let newest = std::fs::read_dir(directory)
        .ok()?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().ends_with("Packages"))
        .filter_map(|entry| entry.metadata().ok()?.modified().ok())
        .max()?;
    let age = now.duration_since(newest).unwrap_or_default();
    i64::try_from(age.as_secs() / 86_400).ok()
}

pub fn update_findings(
    reboot: bool,
    pending: Option<(usize, usize)>,
    lists_days: Option<i64>,
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
    match pending {
        Some((_, security)) if security > 0 => findings.push(Finding::new(
            HealthState::Critical,
            "security_pending",
            format!("{security} security update(s) pending"),
            [security.to_string()],
        )),
        Some((total, _)) if total > 0 => findings.push(Finding::new(
            HealthState::Warning,
            "pending",
            format!("{total} update(s) pending"),
            [total.to_string()],
        )),
        _ => {}
    }
    if let Some(days) = lists_days
        && days > 7
    {
        findings.push(Finding::new(
            HealthState::Warning,
            "search_failed",
            format!("Package lists last refreshed {days} days ago"),
            [days.to_string()],
        ));
    }
    findings
}

fn updates() -> HealthCheck {
    let reboot = Path::new(REBOOT_REQUIRED).exists();
    let pending = pending_updates();
    if pending.is_none() && !reboot {
        return unavailable(UPDATES, "error", "Pending updates could not be listed");
    }
    let lists = lists_age(Path::new(APT_LISTS), SystemTime::now());
    combine(
        UPDATES,
        "current",
        "Updates are current",
        &update_findings(reboot, pending, lists),
    )
}

/// `major:minor` of the device mounted at `/`.
pub fn root_device(mountinfo: &str) -> Option<String> {
    mountinfo.lines().find_map(|line| {
        let fields: Vec<&str> = line.split_whitespace().collect();
        (fields.get(4) == Some(&"/")).then(|| fields.get(2).map(|device| (*device).to_owned()))?
    })
}

/// Whether the block device, or any device below it such as LVM on LUKS, is dm-crypt.
pub fn encrypted(device: &Path, depth: u8) -> bool {
    let uuid = std::fs::read_to_string(device.join("dm").join("uuid")).unwrap_or_default();
    if uuid.starts_with("CRYPT-") {
        return true;
    }
    if depth == 0 {
        return false;
    }
    std::fs::read_dir(device.join("slaves"))
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .any(|entry| encrypted(&entry.path(), depth - 1))
        })
        .unwrap_or(false)
}

fn encryption() -> HealthCheck {
    let Some(device) = std::fs::read_to_string(MOUNTINFO)
        .ok()
        .as_deref()
        .and_then(root_device)
    else {
        return unavailable(
            ENCRYPTION,
            "error",
            "The root file system could not be found",
        );
    };
    if device.starts_with("0:") {
        return unavailable(
            ENCRYPTION,
            "not_supported",
            "The root file system is not on a block device",
        );
    }
    let path = PathBuf::from(BLOCK_DEVICES).join(&device);
    if !path.exists() {
        return unavailable(
            ENCRYPTION,
            "error",
            "The root block device could not be read",
        );
    }
    if encrypted(&path, 8) {
        result(
            ENCRYPTION,
            HealthState::Ok,
            "encrypted",
            "The root file system is on an encrypted device",
            &[],
        )
    } else {
        result(
            ENCRYPTION,
            HealthState::Warning,
            "not_encrypted",
            "The root file system is not encrypted",
            &["/".into()],
        )
    }
}

/// ext4 file systems that recorded errors since they were created or last checked.
pub fn file_system_errors(directory: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut failing: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|entry| {
            std::fs::read_to_string(entry.path().join("errors_count"))
                .ok()
                .and_then(|count| count.trim().parse::<u64>().ok())
                .is_some_and(|count| count > 0)
        })
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    failing.sort();
    failing
}

fn storage(limits: &Thresholds) -> HealthCheck {
    let volumes = volumes();
    if volumes.is_empty() {
        return unavailable(STORAGE, "error", "No file system could be read");
    }
    let mut findings = space(&volumes, limits);
    let failing = file_system_errors(Path::new(EXT4));
    if !failing.is_empty() {
        findings.push(Finding::new(
            HealthState::Warning,
            "fs_errors",
            format!("{} file system(s) recorded errors", failing.len()),
            failing,
        ));
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

    fn scratch(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("midas-health-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn failed_units_are_listed_by_name() {
        let output = "nginx.service loaded failed failed A high performance web server\n\
                      backup.service loaded failed failed Nightly backup\n";
        assert_eq!(failed_units(output), ["nginx.service", "backup.service"]);
        assert!(failed_units("").is_empty());
        assert_eq!(services(&failed_units(output)).code, "stopped");
    }

    #[test]
    fn apt_and_dnf_pending_updates_are_counted() {
        let apt = "Reading package lists...\n\
                   Inst libssl3 [3.0.2-0ubuntu1.14] (3.0.2-0ubuntu1.15 Ubuntu:22.04/jammy-security [amd64])\n\
                   Inst vim [2:8.2] (2:8.2.1 Ubuntu:22.04/jammy-updates [amd64])\n\
                   Conf libssl3 (3.0.2-0ubuntu1.15 Ubuntu:22.04/jammy-security [amd64])\n";
        assert_eq!(apt_pending(apt), (2, 1));
        let dnf = "Last metadata expiration check: 0:10:00 ago.\n\
                   openssl.x86_64    1:3.0.7-25.el9    baseos\n\
                   kernel.x86_64     5.14.0-427.el9    baseos\n";
        assert_eq!(dnf_lines(dnf), 2);
    }

    #[test]
    fn security_updates_are_critical_and_restarts_warn() {
        let codes = |findings: Vec<Finding>| -> Vec<(HealthState, &'static str)> {
            findings
                .into_iter()
                .map(|finding| (finding.state, finding.code))
                .collect()
        };
        assert!(update_findings(false, Some((0, 0)), Some(1)).is_empty());
        assert_eq!(
            codes(update_findings(true, Some((5, 2)), Some(9))),
            [
                (HealthState::Warning, "reboot_pending"),
                (HealthState::Critical, "security_pending"),
                (HealthState::Warning, "search_failed"),
            ]
        );
        assert_eq!(
            codes(update_findings(false, Some((3, 0)), None)),
            [(HealthState::Warning, "pending")]
        );
    }

    #[test]
    fn the_root_device_comes_from_mountinfo() {
        let mountinfo = "22 1 253:1 / / rw,relatime shared:1 - ext4 /dev/mapper/vg-root rw\n\
                         23 22 0:21 / /proc rw - proc proc rw\n";
        assert_eq!(root_device(mountinfo).as_deref(), Some("253:1"));
        assert_eq!(root_device("23 22 0:21 / /proc rw - proc proc rw\n"), None);
    }

    #[test]
    fn luks_below_lvm_counts_as_encrypted() {
        let root = scratch("luks");
        let lvm = root.join("dm-1");
        let crypt = root.join("dm-0");
        std::fs::create_dir_all(lvm.join("dm")).unwrap();
        std::fs::write(lvm.join("dm").join("uuid"), "LVM-abc").unwrap();
        std::fs::create_dir_all(lvm.join("slaves").join("dm-0").join("dm")).unwrap();
        std::fs::write(
            lvm.join("slaves").join("dm-0").join("dm").join("uuid"),
            "CRYPT-LUKS2-x",
        )
        .unwrap();
        std::fs::create_dir_all(crypt.join("dm")).unwrap();
        assert!(encrypted(&lvm, 8));
        assert!(!encrypted(&lvm, 0));
        let plain = root.join("sda2");
        std::fs::create_dir_all(&plain).unwrap();
        assert!(!encrypted(&plain, 8));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn ext4_error_counters_are_reported() {
        let root = scratch("ext4");
        for (device, count) in [("sda1", "0\n"), ("sdb1", "3\n")] {
            std::fs::create_dir_all(root.join(device)).unwrap();
            std::fs::write(root.join(device).join("errors_count"), count).unwrap();
        }
        assert_eq!(file_system_errors(&root), ["sdb1"]);
        assert!(file_system_errors(&root.join("missing")).is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn package_list_age_uses_the_newest_list() {
        let root = scratch("lists");
        std::fs::write(
            root.join("deb.debian.org_dists_bookworm_main_binary-amd64_Packages"),
            "",
        )
        .unwrap();
        let later = SystemTime::now() + Duration::from_secs(10 * 86_400);
        assert_eq!(lists_age(&root, later), Some(10));
        assert_eq!(lists_age(&root.join("missing"), later), None);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn antivirus_is_reported_as_unavailable() {
        let checks = check(&Thresholds::default());
        let antivirus = checks.iter().find(|check| check.id == ANTIVIRUS).unwrap();
        assert_eq!(antivirus.state, HealthState::Unavailable as i32);
        assert_eq!(checks.len(), 5);
    }
}
