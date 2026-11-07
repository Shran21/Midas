//! Directory boundary rules shared by the server and the connector.

pub const MAX_BASE_DNS: usize = 10;
pub const MAX_DN_CHARS: usize = 512;

/// `corp.example` becomes `dc=corp,dc=example`.
pub fn domain_dn(domain: &str) -> Option<String> {
    let domain = domain.trim().to_ascii_lowercase();
    let labels: Vec<_> = domain.split('.').collect();
    let valid = domain.len() <= 253
        && labels.len() >= 2
        && labels.iter().all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        });
    valid.then(|| {
        labels
            .iter()
            .map(|label| format!("dc={label}"))
            .collect::<Vec<_>>()
            .join(",")
    })
}

fn normalize(dn: &str) -> Option<String> {
    let parts: Vec<String> = dn
        .split(',')
        .map(|part| {
            let (key, value) = part.split_once('=')?;
            let (key, value) = (key.trim(), value.trim());
            (!key.is_empty() && !value.is_empty())
                .then(|| format!("{}={}", key.to_ascii_lowercase(), value.to_lowercase()))
        })
        .collect::<Option<_>>()?;
    (!dn.contains('\\') && !parts.is_empty()).then(|| parts.join(","))
}

/// True when `dn` is `base` or below it. Escaped DNs are refused rather than parsed.
pub fn within(dn: &str, base: &str) -> bool {
    match (normalize(dn), normalize(base)) {
        (Some(dn), Some(base)) => dn == base || dn.ends_with(&format!(",{base}")),
        _ => false,
    }
}

/// A usable boundary: one to ten organizational units inside the domain.
pub fn valid_bases(domain: &str, bases: &[String]) -> bool {
    let Some(domain) = domain_dn(domain) else {
        return false;
    };
    !bases.is_empty()
        && bases.len() <= MAX_BASE_DNS
        && bases
            .iter()
            .all(|base| base.chars().count() <= MAX_DN_CHARS && within(base, &domain))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domains_map_to_component_names() {
        assert_eq!(
            domain_dn("Corp.Example").as_deref(),
            Some("dc=corp,dc=example")
        );
        for invalid in [
            "example",
            "-a.example",
            "a..example",
            "a b.example",
            "x.example/",
        ] {
            assert_eq!(domain_dn(invalid), None, "{invalid}");
        }
    }

    #[test]
    fn boundaries_match_whole_components() {
        let base = "OU=Workstations,DC=corp,DC=example";
        assert!(within("CN=PC-01, OU=Workstations,DC=Corp,DC=Example", base));
        assert!(within(base, base));
        assert!(!within("CN=PC-02,OU=Servers,DC=corp,DC=example", base));
        assert!(!within(
            "CN=PC-03,OU=OldWorkstations,DC=corp,DC=example",
            base
        ));
        assert!(!within("CN=a\\,OU=Workstations,DC=corp,DC=example", base));
        assert!(!within("not a dn", base));
    }

    #[test]
    fn bases_stay_inside_the_domain() {
        let inside = vec!["OU=Workstations,DC=corp,DC=example".to_owned()];
        assert!(valid_bases("corp.example", &inside));
        assert!(!valid_bases("corp.example", &[]));
        assert!(!valid_bases(
            "corp.example",
            &["OU=X,DC=other,DC=example".into()]
        ));
        assert!(!valid_bases("corp.example", &vec![inside[0].clone(); 11]));
        assert!(!valid_bases("invalid", &inside));
    }
}
