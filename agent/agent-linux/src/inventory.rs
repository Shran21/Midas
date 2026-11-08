use midas_agent_core::inventory::{Snapshot, common, insert};

const DPKG_STATUS: &str = "/var/lib/dpkg/status";
const GROUPS: &str = "/etc/group";
const ADMIN_GROUPS: [&str; 3] = ["sudo", "wheel", "admin"];

/// Common inventory plus installed packages and administrative users.
pub fn collect() -> Snapshot {
    let mut snapshot = common();
    let packages = match std::fs::read_to_string(DPKG_STATUS) {
        Ok(status) => dpkg(&status),
        Err(_) => rpm(),
    };
    for (name, version) in packages {
        insert(&mut snapshot, "software", &name, version);
    }
    let groups = std::fs::read_to_string(GROUPS).unwrap_or_default();
    for user in administrators(&groups) {
        insert(&mut snapshot, "admin", &user, "");
    }
    snapshot
}

/// Installed packages from a dpkg status file, as (name, "version architecture").
pub fn dpkg(status: &str) -> Vec<(String, String)> {
    let mut packages = Vec::new();
    for paragraph in status.split("\n\n") {
        let field = |name: &str| {
            paragraph
                .lines()
                .find_map(|line| line.strip_prefix(name)?.strip_prefix(':'))
                .map(str::trim)
        };
        let installed = field("Status").is_some_and(|status| status.ends_with(" installed"));
        if let (true, Some(name), Some(version)) = (installed, field("Package"), field("Version")) {
            let architecture = field("Architecture").unwrap_or("");
            packages.push((
                name.to_owned(),
                format!("{version} {architecture}").trim().to_owned(),
            ));
        }
    }
    packages
}

fn rpm() -> Vec<(String, String)> {
    let output = std::process::Command::new("rpm")
        .args([
            "-qa",
            "--queryformat",
            "%{NAME}\t%{VERSION}-%{RELEASE}\t%{ARCH}\n",
        ])
        .output();
    match output {
        Ok(output) if output.status.success() => rpm_list(&String::from_utf8_lossy(&output.stdout)),
        _ => {
            tracing::warn!("no package database could be read");
            Vec::new()
        }
    }
}

/// Parses `rpm -qa` output with tab-separated name, version-release and architecture.
pub fn rpm_list(output: &str) -> Vec<(String, String)> {
    output
        .lines()
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let name = fields.next()?.trim();
            let version = fields.next()?.trim();
            let architecture = fields.next().unwrap_or("").trim();
            (!name.is_empty()).then(|| {
                (
                    name.to_owned(),
                    format!("{version} {architecture}").trim().to_owned(),
                )
            })
        })
        .collect()
}

/// `root` plus members of the administrative groups in an `/etc/group` file.
pub fn administrators(groups: &str) -> Vec<String> {
    let mut users = vec!["root".to_owned()];
    for line in groups.lines() {
        let mut fields = line.split(':');
        let (Some(group), _, _, Some(members)) =
            (fields.next(), fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        if ADMIN_GROUPS.contains(&group) {
            users.extend(
                members
                    .split(',')
                    .map(str::trim)
                    .filter(|user| !user.is_empty())
                    .map(str::to_owned),
            );
        }
    }
    users.sort();
    users.dedup();
    users
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dpkg_lists_only_installed_packages() {
        let status = "Package: openssl\nStatus: install ok installed\nArchitecture: amd64\nVersion: 3.0.13-0ubuntu3\n\n\
                      Package: removed\nStatus: deinstall ok config-files\nVersion: 1.0\n\n\
                      Package: tzdata\nStatus: install ok installed\nArchitecture: all\nVersion: 2024a\n";
        assert_eq!(
            dpkg(status),
            [
                ("openssl".to_owned(), "3.0.13-0ubuntu3 amd64".to_owned()),
                ("tzdata".to_owned(), "2024a all".to_owned()),
            ]
        );
    }

    #[test]
    fn rpm_output_is_parsed() {
        let output = "openssl\t3.0.7-27.el9\tx86_64\nbash\t5.1.8-9.el9\tx86_64\n\n";
        assert_eq!(
            rpm_list(output),
            [
                ("openssl".to_owned(), "3.0.7-27.el9 x86_64".to_owned()),
                ("bash".to_owned(), "5.1.8-9.el9 x86_64".to_owned()),
            ]
        );
    }

    #[test]
    fn administrators_come_from_root_and_admin_groups() {
        let groups =
            "root:x:0:\nsudo:x:27:alice,bob\nwheel:x:10:carol\nusers:x:100:dave\nadmin:x:115:\n";
        assert_eq!(administrators(groups), ["alice", "bob", "carol", "root"]);
    }
}
