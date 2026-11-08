//! Local development only: simulated agents that enroll and stay connected like real ones.
//!
//! `dev_fleet ca` writes an agent CA into `MIDAS_DEV_DIR`. `dev_fleet run` enrolls the devices
//! of every tenant with preapproved tokens (stored once through `MIDAS_ADMIN_DATABASE_URL`) and
//! then reports metrics, inventory and health checks over mTLS until stopped.
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use midas_agent_core::{
    connection::{Settings, Status, run},
    enrollment::{EnrollError, Identity, Server, enroll},
    health::{self, Checker, Thresholds},
    inventory::{self, Collector, Snapshot, insert},
    metrics::{self, Buffer},
};
use midas_protocol::{
    HealthState, Sample,
    enrollment::{EnrollAnswer, EnrollRequest, Kind, Platform},
};
use rcgen::{BasicConstraints, CertificateParams, DnType, IsCa, KeyPair};
use rustls::{ClientConfig, RootCertStore, pki_types::CertificateDer, pki_types::pem::PemObject};
use sha2::{Digest, Sha256};
use sqlx::{Connection, PgConnection};
use tokio::sync::watch;
use uuid::Uuid;

type Error = Box<dyn std::error::Error + Send + Sync>;

#[tokio::main]
async fn main() -> Result<(), Error> {
    let directory = PathBuf::from(std::env::var("MIDAS_DEV_DIR")?);
    match std::env::args().nth(1).as_deref() {
        Some("ca") => authority(&directory),
        Some("run") => fleet(&directory).await,
        _ => Err("usage: dev_fleet ca|run".into()),
    }
}

fn authority(directory: &Path) -> Result<(), Error> {
    let certificate = directory.join("agent-ca.pem");
    if certificate.exists() {
        return Ok(());
    }
    let key = KeyPair::generate()?;
    let mut params = CertificateParams::default();
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params
        .distinguished_name
        .push(DnType::CommonName, "MIDAS local development agent CA");
    let signed = params.self_signed(&key)?;
    fs::write(directory.join("agent-ca-key.pem"), key.serialize_pem())?;
    fs::write(certificate, signed.pem())?;
    println!("Created the local agent CA");
    Ok(())
}

/// One simulated machine; the index decides its role and the problems it reports.
struct Profile {
    name: String,
    tenant: Uuid,
    site: Option<Uuid>,
    platform: Platform,
    kind: Kind,
    /// Position within its tenant; it decides the role and the problems it reports.
    index: usize,
    /// Disconnects after a short while, so the panel shows an offline device.
    leaves: bool,
}

async fn fleet(directory: &Path) -> Result<(), Error> {
    let size: usize = std::env::var("MIDAS_FLEET_SIZE")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(6)
        .clamp(1, 2000);
    let web: u16 = std::env::var("MIDAS_FLEET_WEB_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(7443);
    let agent: u16 = std::env::var("MIDAS_FLEET_AGENT_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(7444);
    let state = directory.join("fleet");
    fs::create_dir_all(&state)?;
    let mut admin = PgConnection::connect(&std::env::var("MIDAS_ADMIN_DATABASE_URL")?).await?;
    let tenants: Vec<(Uuid, String, Option<Uuid>)> = sqlx::query_as("SELECT t.id, t.name, (SELECT s.id FROM midas.sites s WHERE s.tenant_id = t.id ORDER BY s.name LIMIT 1) FROM midas.tenants t ORDER BY t.name")
        .fetch_all(&mut admin).await?;
    let mut roots = RootCertStore::empty();
    for certificate in CertificateDer::pem_file_iter(directory.join("tls.pem"))? {
        roots.add(certificate?)?;
    }
    let verifying = Arc::new(
        ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_protocol_versions(&[&rustls::version::TLS13])?
            .with_root_certificates(roots.clone())
            .with_no_client_auth(),
    );
    let server = Server {
        host: "127.0.0.1".into(),
        port: web,
        tls: verifying,
        timeout: Duration::from_secs(10),
    };
    let mut handles = Vec::new();
    for index in 0..size {
        let (tenant, tenant_name, site) = &tenants[index % tenants.len()];
        let prefix: String = tenant_name
            .chars()
            .map(|c| match c.to_ascii_lowercase() {
                'á' | 'Á' => 'a',
                'é' | 'É' => 'e',
                'í' | 'Í' => 'i',
                'ó' | 'Ó' | 'ö' | 'Ö' | 'ő' | 'Ő' => 'o',
                'ú' | 'Ú' | 'ü' | 'Ü' | 'ű' | 'Ű' => 'u',
                other => other,
            })
            .filter(char::is_ascii_alphanumeric)
            .take(4)
            .collect::<String>()
            .to_uppercase();
        let slot = index / tenants.len();
        let (platform, kind, role) = match slot % 6 {
            1 => (Platform::Windows, Kind::Server, "SRV"),
            3 => (Platform::Linux, Kind::Server, "LNX"),
            5 => (Platform::Linux, Kind::Workstation, "DEB"),
            _ => (Platform::Windows, Kind::Workstation, "WS"),
        };
        let profile = Profile {
            name: format!("{role}-{prefix}-{:03}", index + 1),
            tenant: *tenant,
            site: if slot % 4 == 3 { None } else { *site },
            platform,
            kind,
            index: slot,
            leaves: index + 1 == size && size > 2,
        };
        let (identity, certificate) = match enrolled(&state, &profile, &mut admin, &server).await {
            Ok(pair) => pair,
            Err(error) => {
                eprintln!("{}: enrollment failed: {error}", profile.name);
                continue;
            }
        };
        let tls = identity.client_config(roots.clone(), &certificate)?;
        handles.push(tokio::spawn(simulate(profile, tls, agent, state.clone())));
    }
    println!(
        "{} simulated agents running; stop with Ctrl+C",
        handles.len()
    );
    tokio::signal::ctrl_c().await?;
    Ok(())
}

async fn enrolled(
    state: &Path,
    profile: &Profile,
    admin: &mut PgConnection,
    server: &Server,
) -> Result<(Identity, String), Error> {
    let key = state.join(format!("{}.key.pem", profile.name));
    let certificate = state.join(format!("{}.pem", profile.name));
    if key.exists() && certificate.exists() {
        return Ok((
            Identity::from_pem(&fs::read_to_string(key)?)?,
            fs::read_to_string(certificate)?,
        ));
    }
    let mut secret = [0u8; 32];
    getrandom::fill(&mut secret)?;
    let token = format!(
        "midas-enroll-{}",
        secret
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    );
    let mut digest = Sha256::new();
    digest.update(b"enrollment-token");
    digest.update([0]);
    digest.update(token.as_bytes());
    let (platform, kind) = (
        match profile.platform {
            Platform::Windows => "windows",
            Platform::Linux => "linux",
        },
        match profile.kind {
            Kind::Workstation => "workstation",
            Kind::Server => "server",
        },
    );
    sqlx::query("INSERT INTO midas.enrollment_tokens (digest, id, tenant_id, site_id, platform, kind, preapproved, expires_at) VALUES ($1, $2, $3, $4, $5, $6, true, clock_timestamp() + interval '10 minutes')")
        .bind(digest.finalize().to_vec()).bind(Uuid::new_v4()).bind(profile.tenant).bind(profile.site)
        .bind(platform).bind(kind).execute(&mut *admin).await?;
    let identity = Identity::generate()?;
    let request = EnrollRequest {
        token,
        csr: identity.signing_request()?,
        platform: profile.platform,
        kind: profile.kind,
        name: profile.name.clone(),
    };
    // The server may still be starting.
    let mut attempt = 0;
    let answer = loop {
        match enroll(server, &request).await {
            Err(EnrollError::Unreachable | EnrollError::Unavailable) if attempt < 60 => {
                attempt += 1;
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
            result => break result.map_err(|error| format!("{error:?}"))?,
        }
    };
    let EnrollAnswer::Issued(issued) = answer else {
        return Err("a preapproved token must issue a certificate".into());
    };
    fs::write(key, identity.key_pem().as_bytes())?;
    fs::write(certificate, &issued.certificate)?;
    Ok((identity, issued.certificate))
}

/// A small deterministic generator, so each device keeps its own pattern.
struct Noise(u64);

impl Noise {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
}

const GIB: u64 = 1 << 30;

fn sample(at: i64, noise: &mut Noise, profile: &Profile) -> Sample {
    let memory_total = if profile.kind == Kind::Server { 32 } else { 16 } * GIB;
    let disk_total = if profile.kind == Kind::Server {
        512
    } else {
        256
    } * GIB;
    let disk_share = if profile.index % 6 == 1 {
        93
    } else {
        40 + (profile.index as u64 * 7) % 40
    };
    Sample {
        at,
        cpu_permille: (60 + noise.next() % 300 + (profile.index as u64 * 37) % 200) as u32,
        memory_used: memory_total / 100 * (35 + noise.next() % 30),
        memory_total,
        disk_used: disk_total / 100 * disk_share,
        disk_total,
        net_rx: noise.next() % 5_000_000,
        net_tx: noise.next() % 2_000_000,
    }
}

fn snapshot(profile: &Profile) -> Snapshot {
    let mut snapshot = Snapshot::new();
    insert(&mut snapshot, "hardware", "host_name", &profile.name);
    insert(
        &mut snapshot,
        "hardware",
        "cpu",
        "Intel(R) Core(TM) i5-12500",
    );
    insert(&mut snapshot, "hardware", "logical_cpus", "12");
    let windows = profile.platform == Platform::Windows;
    let (name, version, long) = match (windows, profile.kind) {
        (true, Kind::Server) => ("Windows", "Server 2022", "Windows Server 2022 Datacenter"),
        (true, Kind::Workstation) => ("Windows", "11 (26100)", "Windows 11 Pro 24H2"),
        (false, Kind::Server) => ("Ubuntu", "24.04", "Linux (Ubuntu 24.04 LTS)"),
        (false, Kind::Workstation) => ("Debian", "13", "Linux (Debian GNU/Linux 13)"),
    };
    insert(&mut snapshot, "os", "name", name);
    insert(&mut snapshot, "os", "version", version);
    insert(&mut snapshot, "os", "long_version", long);
    insert(
        &mut snapshot,
        "os",
        "kernel",
        if windows { "26100" } else { "6.8.0-48-generic" },
    );
    insert(
        &mut snapshot,
        "hardware",
        "memory_bytes",
        (16 * GIB).to_string(),
    );
    if windows {
        insert(&mut snapshot, "disk", "C:\\", format!("{} NTFS", 256 * GIB));
        for (package, value) in [
            ("7-Zip 24.08 (x64)", "24.08 | Igor Pavlov"),
            ("Google Chrome", "131.0.6778.86 | Google LLC"),
            ("Microsoft Edge", "131.0.2903.70 | Microsoft Corporation"),
            (
                "Microsoft 365 Apps for business",
                "16.0.18129 | Microsoft Corporation",
            ),
            ("Mozilla Firefox (x64 hu)", "133.0 | Mozilla"),
            ("Notepad++ (64-bit x64)", "8.7.1 | Notepad++ Team"),
            ("VLC media player", "3.0.21 | VideoLAN"),
        ] {
            insert(&mut snapshot, "software", package, value);
        }
        insert(&mut snapshot, "admin", "Administrator", "local");
        insert(&mut snapshot, "admin", "it.support", "local");
    } else {
        insert(&mut snapshot, "disk", "/", format!("{} ext4", 128 * GIB));
        for (package, value) in [
            ("bash", "5.2.21-2ubuntu4 amd64"),
            ("openssh-server", "1:9.6p1-3ubuntu13 amd64"),
            ("nginx", "1.24.0-2ubuntu7 amd64"),
            ("postgresql-16", "16.4-0ubuntu0.24.04.2 amd64"),
            ("systemd", "255.4-1ubuntu8 amd64"),
        ] {
            insert(&mut snapshot, "software", package, value);
        }
        insert(&mut snapshot, "admin", "root", "uid 0");
        insert(&mut snapshot, "admin", "ops", "sudo");
    }
    snapshot
}

fn checker(profile: &Profile) -> Checker {
    let (index, windows) = (profile.index, profile.platform == Platform::Windows);
    Arc::new(move |limits: &Thresholds| {
        let check = |id: &str, state, code: &str, summary: &str, subjects: &[&str]| {
            health::result(
                id,
                state,
                code,
                summary,
                &subjects.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>(),
            )
        };
        let mut checks = vec![
            if index % 6 == 4 {
                check(
                    "services",
                    HealthState::Critical,
                    "stopped",
                    "1 service(s) stopped",
                    &["Spooler"],
                )
            } else {
                check(
                    "services",
                    HealthState::Ok,
                    "running",
                    "All automatic services are running",
                    &[],
                )
            },
            if index % 6 == 2 {
                check(
                    "updates",
                    HealthState::Warning,
                    "reboot_pending",
                    "Restart pending",
                    &[],
                )
            } else {
                check(
                    "updates",
                    HealthState::Ok,
                    "current",
                    "Updates are current",
                    &[],
                )
            },
            if index % 6 == 1 {
                check(
                    "storage",
                    HealthState::Warning,
                    "low_space",
                    &format!("Low free space (below {}%)", limits.low_space_percent),
                    &["C:\\"],
                )
            } else {
                check(
                    "storage",
                    HealthState::Ok,
                    "healthy",
                    "Enough free space",
                    &[],
                )
            },
        ];
        if windows {
            checks.push(check(
                "antivirus",
                HealthState::Ok,
                "protected",
                "Microsoft Defender is protecting",
                &[],
            ));
            checks.push(check(
                "encryption",
                HealthState::Ok,
                "encrypted",
                "BitLocker protects every fixed volume",
                &[],
            ));
        } else {
            checks.push(check(
                "antivirus",
                HealthState::Unavailable,
                "not_supported",
                "No antivirus check on Linux",
                &[],
            ));
            checks.push(check(
                "encryption",
                HealthState::Warning,
                "not_encrypted",
                "The root file system is not encrypted",
                &["/"],
            ));
        }
        checks
    })
}

async fn simulate(profile: Profile, tls: Arc<ClientConfig>, port: u16, state: PathBuf) {
    let mut noise = Noise(profile.name.bytes().fold(17, |seed, byte| {
        seed.wrapping_mul(31).wrapping_add(u64::from(byte))
    }));
    let buffer = Buffer::shared(2_880);
    let now = time::OffsetDateTime::now_utc().unix_timestamp() / 60 * 60;
    for minute in (1..=60).rev() {
        metrics::lock(&buffer).push(sample(now - minute * 60, &mut noise, &profile));
    }
    let inventory_state = inventory::shared(state.join(format!("{}.inventory.pb", profile.name)));
    let health_state = health::shared();
    let platform = match profile.platform {
        Platform::Windows => "windows",
        Platform::Linux => "linux",
    };
    let settings = Settings {
        metrics: Some(buffer.clone()),
        inventory: Some(inventory_state.clone()),
        health: Some(health_state.clone()),
        capabilities: [
            "metrics",
            "inventory",
            "health.services",
            "health.updates",
            "health.storage",
        ]
        .map(String::from)
        .to_vec(),
        ..Settings::new("127.0.0.1".into(), port, tls, platform)
    };
    let (status, mut watched) = watch::channel(Status::Connecting { attempt: 0 });
    let (stop, stopped) = watch::channel(false);
    let collector: Collector = {
        let snapshot = snapshot(&profile);
        Arc::new(move || snapshot.clone())
    };
    let delay = Duration::from_secs(profile.index as u64 % 20);
    tokio::spawn(inventory::schedule(
        collector,
        inventory_state,
        delay,
        stopped.clone(),
    ));
    tokio::spawn(health::schedule(
        checker(&profile),
        health_state,
        delay,
        stopped.clone(),
    ));
    tokio::spawn(run(settings, status, stopped));
    let name = profile.name.clone();
    tokio::spawn(async move {
        while watched.changed().await.is_ok() {
            if let Status::Connected { .. } = *watched.borrow() {
                println!("{name}: connected");
            }
        }
    });
    if profile.leaves {
        tokio::time::sleep(Duration::from_secs(90)).await;
        println!("{}: disconnected on purpose", profile.name);
        stop.send_replace(true);
        return;
    }
    let _running = stop;
    let mut tick = tokio::time::interval(Duration::from_secs(60));
    tick.tick().await;
    loop {
        tick.tick().await;
        let at = time::OffsetDateTime::now_utc().unix_timestamp() / 60 * 60;
        metrics::lock(&buffer).push(sample(at, &mut noise, &profile));
    }
}
