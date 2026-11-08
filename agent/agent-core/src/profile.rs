//! What kind of host the agent runs on. The server offers graphical tools only on hosts with a
//! graphical desktop; Server Core and other headless hosts get terminal and background tools.

pub const DESKTOP: &str = "host.desktop";
pub const HEADLESS: &str = "host.headless";
pub const SERVER: &str = "host.server";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Profile {
    /// `None` when the platform cannot tell; the server then shows the desktop as unknown.
    pub desktop: Option<bool>,
    pub server: bool,
}

impl Profile {
    /// From the Windows `InstallationType` value: Client, Server, Server Core or Nano Server.
    pub fn windows(installation: Option<&str>) -> Self {
        match installation.map(str::trim) {
            Some(value) if value.eq_ignore_ascii_case("client") => Self {
                desktop: Some(true),
                server: false,
            },
            Some(value) if value.eq_ignore_ascii_case("server") => Self {
                desktop: Some(true),
                server: true,
            },
            Some(value)
                if value.eq_ignore_ascii_case("server core")
                    || value.eq_ignore_ascii_case("nano server") =>
            {
                Self {
                    desktop: Some(false),
                    server: true,
                }
            }
            _ => Self::default(),
        }
    }

    /// Graphical when systemd starts `graphical.target` or a display manager is configured.
    /// Without systemd information the desktop stays unknown.
    pub fn linux(default_target: Option<&str>, display_manager: bool) -> Self {
        let desktop = match default_target {
            _ if display_manager => Some(true),
            Some(target) => Some(target == "graphical.target"),
            None => None,
        };
        Self {
            desktop,
            server: false,
        }
    }

    pub fn capabilities(self) -> Vec<String> {
        let mut capabilities = Vec::new();
        match self.desktop {
            Some(true) => capabilities.push(DESKTOP.to_owned()),
            Some(false) => capabilities.push(HEADLESS.to_owned()),
            None => {}
        }
        if self.server {
            capabilities.push(SERVER.to_owned());
        }
        capabilities
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_installation_types() {
        assert_eq!(Profile::windows(Some("Client")).capabilities(), [DESKTOP]);
        assert_eq!(
            Profile::windows(Some("Server")).capabilities(),
            [DESKTOP, SERVER]
        );
        assert_eq!(
            Profile::windows(Some("Server Core")).capabilities(),
            [HEADLESS, SERVER]
        );
        assert_eq!(
            Profile::windows(Some("Nano Server")).capabilities(),
            [HEADLESS, SERVER]
        );
        assert!(Profile::windows(Some("Embedded")).capabilities().is_empty());
        assert!(Profile::windows(None).capabilities().is_empty());
    }

    #[test]
    fn linux_desktop_follows_systemd() {
        assert_eq!(
            Profile::linux(Some("graphical.target"), false).capabilities(),
            [DESKTOP]
        );
        assert_eq!(
            Profile::linux(Some("multi-user.target"), false).capabilities(),
            [HEADLESS]
        );
        assert_eq!(
            Profile::linux(Some("multi-user.target"), true).capabilities(),
            [DESKTOP]
        );
        assert!(Profile::linux(None, false).capabilities().is_empty());
    }
}
