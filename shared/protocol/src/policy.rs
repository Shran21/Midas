//! Settings that customers, sites and groups may override. Both sides use this catalogue,
//! so the server validates exactly what agents understand.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Setting {
    pub key: &'static str,
    pub minimum: i64,
    pub maximum: i64,
    pub default: i64,
}

impl Setting {
    pub const fn clamp(&self, value: i64) -> i64 {
        if value < self.minimum {
            self.minimum
        } else if value > self.maximum {
            self.maximum
        } else {
            value
        }
    }
}

pub const LOW_SPACE_PERCENT: Setting = Setting {
    key: "health.low_space_percent",
    minimum: 1,
    maximum: 50,
    default: 10,
};
/// Never above the low-space limit; agents lower it to that limit.
pub const CRITICAL_SPACE_PERCENT: Setting = Setting {
    key: "health.critical_space_percent",
    minimum: 1,
    maximum: 50,
    default: 5,
};
pub const UPDATE_WARNING_DAYS: Setting = Setting {
    key: "health.update_warning_days",
    minimum: 1,
    maximum: 365,
    default: 35,
};
/// Never below the warning limit; agents raise it to that limit.
pub const UPDATE_CRITICAL_DAYS: Setting = Setting {
    key: "health.update_critical_days",
    minimum: 1,
    maximum: 365,
    default: 60,
};
pub const SIGNATURE_WARNING_DAYS: Setting = Setting {
    key: "health.signature_warning_days",
    minimum: 1,
    maximum: 90,
    default: 3,
};
/// Never below the warning limit; agents raise it to that limit.
pub const SIGNATURE_CRITICAL_DAYS: Setting = Setting {
    key: "health.signature_critical_days",
    minimum: 1,
    maximum: 90,
    default: 7,
};

/// Server-side: how long a device may stay offline before an alert opens.
pub const OFFLINE_ALERT_MINUTES: Setting = Setting {
    key: "alerts.offline_minutes",
    minimum: 1,
    maximum: 1440,
    default: 5,
};

pub const SETTINGS: [Setting; 7] = [
    LOW_SPACE_PERCENT,
    CRITICAL_SPACE_PERCENT,
    UPDATE_WARNING_DAYS,
    UPDATE_CRITICAL_DAYS,
    SIGNATURE_WARNING_DAYS,
    SIGNATURE_CRITICAL_DAYS,
    OFFLINE_ALERT_MINUTES,
];

pub fn find(key: &str) -> Option<&'static Setting> {
    SETTINGS.iter().find(|setting| setting.key == key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_unique_and_defaults_in_range() {
        for (index, setting) in SETTINGS.iter().enumerate() {
            assert_eq!(setting.clamp(setting.default), setting.default);
            assert!(
                SETTINGS[..index]
                    .iter()
                    .all(|other| other.key != setting.key)
            );
            assert_eq!(find(setting.key), Some(setting));
        }
        assert_eq!(LOW_SPACE_PERCENT.clamp(0), 1);
        assert_eq!(LOW_SPACE_PERCENT.clamp(99), 50);
        assert_eq!(find("health.unknown"), None);
    }
}
