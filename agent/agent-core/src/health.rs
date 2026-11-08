use std::{
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, Instant},
};

use midas_protocol::{
    HealthCheck, HealthReport, HealthState, MAX_HEALTH_CHECKS, MAX_HEALTH_SUBJECTS, MAX_HEALTH_TEXT,
};
use prost::Message;
use tokio::sync::watch;

/// Platform-specific checks; they may block, so they run on a blocking thread.
pub type Checker = Arc<dyn Fn() -> Vec<HealthCheck> + Send + Sync>;

pub const CHECK_EVERY: Duration = Duration::from_secs(5 * 60);
/// An unchanged result is resent this often so the server knows it is current.
pub const REFRESH_EVERY: Duration = Duration::from_secs(3600);

pub const SERVICES: &str = "services";
pub const UPDATES: &str = "updates";
pub const ANTIVIRUS: &str = "antivirus";
pub const ENCRYPTION: &str = "encryption";
pub const STORAGE: &str = "storage";

/// Below this share of free space a volume is low, unless it still has `ROOMY` bytes free.
const LOW_PERMILLE: u64 = 100;
const CRITICAL_PERMILLE: u64 = 50;
const ROOMY: u64 = 100 * 1024 * 1024 * 1024;
const REPORT_BYTES: usize = midas_protocol::MAX_MESSAGE_BYTES / 2;
/// Read-only images that are always full, such as snap packages.
const IMAGE_FILE_SYSTEMS: [&str; 4] = ["squashfs", "iso9660", "udf", "erofs"];

fn bounded(text: &str) -> String {
    text.trim().chars().take(MAX_HEALTH_TEXT).collect()
}

/// One reason a check is not healthy; the most severe one names the check's state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub state: HealthState,
    pub code: &'static str,
    pub summary: String,
    pub subjects: Vec<String>,
}

impl Finding {
    pub fn new(
        state: HealthState,
        code: &'static str,
        summary: impl Into<String>,
        subjects: impl IntoIterator<Item = String>,
    ) -> Self {
        Self {
            state,
            code,
            summary: summary.into(),
            subjects: subjects.into_iter().collect(),
        }
    }
}

pub fn result(
    id: &str,
    state: HealthState,
    code: &str,
    summary: &str,
    subjects: &[String],
) -> HealthCheck {
    HealthCheck {
        id: id.into(),
        state: state as i32,
        code: code.into(),
        summary: bounded(summary),
        subjects: subjects
            .iter()
            .map(|subject| bounded(subject))
            .filter(|subject| !subject.is_empty())
            .take(MAX_HEALTH_SUBJECTS)
            .collect(),
    }
}

/// The check cannot run here; the panel shows it as unavailable instead of hiding it.
pub fn unavailable(id: &str, code: &str, summary: &str) -> HealthCheck {
    result(id, HealthState::Unavailable, code, summary, &[])
}

/// The most severe finding decides the state and code; every summary is kept.
pub fn combine(id: &str, ok_code: &str, ok_summary: &str, findings: &[Finding]) -> HealthCheck {
    let Some(worst) = findings
        .iter()
        .fold(None::<&Finding>, |worst, finding| match worst {
            Some(current) if current.state >= finding.state => Some(current),
            _ => Some(finding),
        })
    else {
        return result(id, HealthState::Ok, ok_code, ok_summary, &[]);
    };
    let summary = findings
        .iter()
        .map(|finding| finding.summary.as_str())
        .collect::<Vec<_>>()
        .join("; ");
    result(id, worst.state, worst.code, &summary, &worst.subjects)
}

/// Automatic services that are not running although they should be.
pub fn services(failed: &[String]) -> HealthCheck {
    let findings = if failed.is_empty() {
        Vec::new()
    } else {
        vec![Finding::new(
            HealthState::Critical,
            "stopped",
            format!("{} service(s) stopped unexpectedly", failed.len()),
            failed.iter().cloned(),
        )]
    };
    combine(
        SERVICES,
        "running",
        "All automatic services are running",
        &findings,
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Volume {
    pub name: String,
    pub total: u64,
    pub available: u64,
}

/// Fixed volumes with a real file system.
pub fn volumes() -> Vec<Volume> {
    let Ok(disks) = sysinfo::Disks::new_with_refreshed_list() else {
        return Vec::new();
    };
    disks
        .iter()
        .filter(|disk| {
            !disk.is_removable()
                && disk.total_space() > 0
                && !IMAGE_FILE_SYSTEMS
                    .iter()
                    .any(|image| disk.file_system().eq_ignore_ascii_case(image))
        })
        .map(|disk| Volume {
            name: disk.mount_point().to_string_lossy().into_owned(),
            total: disk.total_space(),
            available: disk.available_space(),
        })
        .collect()
}

/// Low free space as findings: below 10 % a warning, below 5 % critical.
pub fn space(volumes: &[Volume]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (state, limit) in [
        (HealthState::Critical, CRITICAL_PERMILLE),
        (HealthState::Warning, LOW_PERMILLE),
    ] {
        let low: Vec<String> = volumes
            .iter()
            .filter(|volume| {
                let permille = volume.available.saturating_mul(1000) / volume.total.max(1);
                let previous = if state == HealthState::Warning {
                    CRITICAL_PERMILLE
                } else {
                    0
                };
                volume.available < ROOMY && permille < limit && permille >= previous
            })
            .map(|volume| {
                let percent = volume.available.saturating_mul(100) / volume.total.max(1);
                format!("{} ({percent}% free)", volume.name)
            })
            .collect();
        if !low.is_empty() {
            findings.push(Finding::new(
                state,
                "low_space",
                format!("{} volume(s) below {}% free space", low.len(), limit / 10),
                low,
            ));
        }
    }
    findings
}

/// Whole days between two Unix times, never negative.
pub fn days_between(earlier: i64, later: i64) -> i64 {
    later.saturating_sub(earlier).max(0) / 86_400
}

#[derive(Default)]
pub struct Latest {
    pub checks: Vec<HealthCheck>,
    /// Grows with every result worth sending; zero before the first run.
    pub revision: u64,
    published: Option<Instant>,
}

pub type Shared = Arc<Mutex<Latest>>;

/// Drops subjects from the longest lists until the report is far below the message limit;
/// many long multi-byte names could otherwise exceed it.
fn fit(checks: &mut [HealthCheck]) {
    let size = |checks: &[HealthCheck]| {
        HealthReport {
            sequence: u64::MAX,
            checks: checks.to_vec(),
        }
        .encoded_len()
    };
    while size(checks) > REPORT_BYTES {
        let Some(longest) = checks
            .iter_mut()
            .filter(|check| !check.subjects.is_empty())
            .max_by_key(|check| check.subjects.len())
        else {
            return;
        };
        longest.subjects.pop();
    }
}

pub fn shared() -> Shared {
    Arc::new(Mutex::new(Latest::default()))
}

pub fn lock(shared: &Shared) -> MutexGuard<'_, Latest> {
    shared
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Latest {
    /// Keeps a new result; an unchanged one is republished only after `REFRESH_EVERY`.
    pub fn publish(&mut self, mut checks: Vec<HealthCheck>, now: Instant) -> bool {
        checks.truncate(MAX_HEALTH_CHECKS);
        fit(&mut checks);
        let fresh = self
            .published
            .is_some_and(|at| now.saturating_duration_since(at) < REFRESH_EVERY);
        if self.revision > 0 && fresh && checks == self.checks {
            return false;
        }
        self.checks = checks;
        self.revision += 1;
        self.published = Some(now);
        true
    }

    pub fn report(&self, sequence: u64) -> Option<HealthReport> {
        (self.revision > 0).then(|| HealthReport {
            sequence,
            checks: self.checks.clone(),
        })
    }
}

/// Runs the checks after `first_delay` and then every five minutes.
pub async fn schedule(
    checker: Checker,
    shared: Shared,
    first_delay: Duration,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut wait = first_delay;
    loop {
        tokio::select! {
            () = tokio::time::sleep(wait) => {}
            _ = shutdown.wait_for(|stop| *stop) => return,
        }
        wait = CHECK_EVERY;
        let check = checker.clone();
        if let Ok(checks) = tokio::task::spawn_blocking(move || check()).await {
            lock(&shared).publish(checks, Instant::now());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(check: &HealthCheck) -> HealthState {
        HealthState::try_from(check.state).unwrap()
    }

    #[test]
    fn stopped_services_are_critical_and_named() {
        assert_eq!(state(&services(&[])), HealthState::Ok);
        let names: Vec<String> = (0..20).map(|i| format!("service{i}")).collect();
        let check = services(&names);
        assert_eq!(
            (state(&check), check.code.as_str()),
            (HealthState::Critical, "stopped")
        );
        assert_eq!(check.subjects.len(), MAX_HEALTH_SUBJECTS);
        assert!(check.summary.starts_with("20 service(s)"));
    }

    #[test]
    fn the_most_severe_finding_wins_and_every_summary_stays() {
        let findings = [
            Finding::new(
                HealthState::Warning,
                "reboot_pending",
                "Restart pending",
                [],
            ),
            Finding::new(
                HealthState::Critical,
                "outdated",
                "No update for 70 days",
                ["70".into()],
            ),
            Finding::new(HealthState::Critical, "later", "Same severity", []),
        ];
        let check = combine(UPDATES, "current", "Up to date", &findings);
        assert_eq!(
            (state(&check), check.code.as_str()),
            (HealthState::Critical, "outdated")
        );
        assert_eq!(check.subjects, ["70"]);
        assert_eq!(
            check.summary,
            "Restart pending; No update for 70 days; Same severity"
        );
        assert_eq!(
            combine(UPDATES, "current", "Up to date", &[]).code,
            "current"
        );
    }

    #[test]
    fn low_space_uses_percentages_but_spares_roomy_volumes() {
        let gib = 1024 * 1024 * 1024;
        let volumes = [
            Volume {
                name: "C:\\".into(),
                total: 200 * gib,
                available: 8 * gib,
            },
            Volume {
                name: "D:\\".into(),
                total: 200 * gib,
                available: 16 * gib,
            },
            Volume {
                name: "E:\\".into(),
                total: 200 * gib,
                available: 100 * gib,
            },
            Volume {
                name: "/data".into(),
                total: 4000 * gib,
                available: 150 * gib,
            },
        ];
        let findings = space(&volumes);
        assert_eq!(findings.len(), 2);
        assert_eq!(findings[0].state, HealthState::Critical);
        assert_eq!(findings[0].subjects, ["C:\\ (4% free)"]);
        assert_eq!(findings[1].state, HealthState::Warning);
        assert_eq!(findings[1].subjects, ["D:\\ (8% free)"]);
        let check = combine(STORAGE, "healthy", "Healthy", &findings);
        assert_eq!(
            (state(&check), check.code.as_str()),
            (HealthState::Critical, "low_space")
        );
    }

    #[test]
    fn text_is_bounded_and_empty_subjects_are_dropped() {
        let long = "x".repeat(400);
        let check = result(
            STORAGE,
            HealthState::Warning,
            "low_space",
            &long,
            &[long.clone(), " ".into()],
        );
        assert_eq!(check.summary.chars().count(), MAX_HEALTH_TEXT);
        assert_eq!(check.subjects.len(), 1);
        assert_eq!(check.subjects[0].chars().count(), MAX_HEALTH_TEXT);
    }

    #[test]
    fn unchanged_results_are_republished_only_after_an_hour() {
        let mut latest = Latest::default();
        assert!(latest.report(1).is_none());
        let start = Instant::now();
        let checks = vec![services(&[])];
        assert!(latest.publish(checks.clone(), start));
        assert!(!latest.publish(checks.clone(), start + Duration::from_secs(600)));
        assert_eq!(latest.revision, 1);
        assert!(latest.publish(
            vec![services(&["spooler".into()])],
            start + Duration::from_secs(900)
        ));
        assert!(latest.publish(
            latest.checks.clone(),
            start + Duration::from_secs(900) + REFRESH_EVERY
        ));
        assert_eq!(latest.revision, 3);
        assert_eq!(latest.report(9).unwrap().sequence, 9);
    }

    #[test]
    fn long_multi_byte_reports_stay_below_the_message_limit() {
        let names: Vec<String> = (0..MAX_HEALTH_SUBJECTS)
            .map(|n| format!("{n}{}", "服".repeat(300)))
            .collect();
        let checks: Vec<HealthCheck> = (0..MAX_HEALTH_CHECKS)
            .map(|n| {
                result(
                    &format!("check_{}", char::from(b'a' + n as u8)),
                    HealthState::Critical,
                    "stopped",
                    &"服".repeat(300),
                    &names,
                )
            })
            .collect();
        let mut latest = Latest::default();
        latest.publish(checks, Instant::now());
        let report = latest.report(1).unwrap();
        assert_eq!(report.checks.len(), MAX_HEALTH_CHECKS);
        assert!(report.checks.iter().all(|check| !check.summary.is_empty()));
        let encoded = midas_protocol::encode(midas_protocol::Body::HealthReport(report));
        assert!(encoded.len() < midas_protocol::MAX_MESSAGE_BYTES);
    }

    #[test]
    fn days_never_go_negative() {
        assert_eq!(days_between(0, 3 * 86_400 + 5), 3);
        assert_eq!(days_between(100, 0), 0);
    }

    #[test]
    fn this_machine_has_at_least_one_fixed_volume() {
        assert!(volumes().iter().any(|volume| volume.total > 0));
    }
}
