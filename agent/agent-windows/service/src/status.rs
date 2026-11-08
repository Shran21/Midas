//! What the tray shows for an agent status: a plain-language title and, when the user can act,
//! what to do next.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Ok,
    Working,
    Attention,
    Problem,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct View {
    pub tone: Tone,
    pub title: &'static str,
    pub advice: Option<&'static str>,
}

/// `status` is the JSON the service serves on its pipe; `None` means the service did not answer.
pub fn view(status: Option<&str>, hungarian: bool) -> View {
    let text = |hu: &'static str, en: &'static str| if hungarian { hu } else { en };
    let state = status
        .and_then(|json| serde_json::from_str::<serde_json::Value>(json).ok())
        .and_then(|value| {
            let state = value.get("state")?.as_str()?.to_owned();
            let reason = value
                .get("reason")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned);
            Some((state, reason))
        });
    let Some((state, reason)) = state else {
        return View {
            tone: Tone::Problem,
            title: text(
                "A MIDAS szolgáltatás nem fut",
                "The MIDAS service is not running",
            ),
            advice: Some(text(
                "Indítsd újra a számítógépet. Ha a hiba megmarad, szólj a rendszergazdának.",
                "Restart the computer. If the problem stays, tell your administrator.",
            )),
        };
    };
    match (state.as_str(), reason.as_deref()) {
        ("connected", _) => View {
            tone: Tone::Ok,
            title: text(
                "Kapcsolódva a MIDAS szerverhez",
                "Connected to the MIDAS server",
            ),
            advice: None,
        },
        ("starting", _) | ("connecting", _) => View {
            tone: Tone::Working,
            title: text("Kapcsolódás…", "Connecting…"),
            advice: None,
        },
        ("waiting", Some("unreachable")) | ("waiting", Some("lost")) => View {
            tone: Tone::Attention,
            title: text("A szerver nem érhető el", "The server cannot be reached"),
            advice: Some(text(
                "Ellenőrizd a hálózati vagy internetkapcsolatot. Az agent magától újrapróbálkozik; ha órák múlva sem kapcsolódik, szólj a rendszergazdának.",
                "Check the network or internet connection. The agent retries on its own; if it still cannot connect after hours, tell your administrator.",
            )),
        },
        ("waiting", _) => View {
            tone: Tone::Problem,
            title: text(
                "A kapcsolatot a szerver nem fogadta el",
                "The server did not accept the connection",
            ),
            advice: Some(text(
                "Ezt csak a rendszergazda tudja rendezni (tanúsítvány vagy verzió). Szólj neki.",
                "Only your administrator can fix this (certificate or version). Please tell them.",
            )),
        },
        ("pending", _) => View {
            tone: Tone::Attention,
            title: text("Jóváhagyásra vár", "Waiting for approval"),
            advice: Some(text(
                "A rendszergazdának jóvá kell hagynia ezt a gépet. Teendőd nincs.",
                "Your administrator has to approve this computer. Nothing to do on your side.",
            )),
        },
        ("rejected", _) => View {
            tone: Tone::Problem,
            title: text(
                "A gép regisztrációját elutasították",
                "This computer's enrollment was refused",
            ),
            advice: Some(text(
                "Ha ez a gép kezelt eszköz kell legyen, szólj a rendszergazdának.",
                "If this computer should be managed, tell your administrator.",
            )),
        },
        ("unconfigured", _) | ("unenrolled", _) => View {
            tone: Tone::Problem,
            title: text("Az agent nincs beállítva", "The agent is not set up"),
            advice: Some(text(
                "A telepítés nem fejeződött be. Szólj a rendszergazdának.",
                "The installation is incomplete. Tell your administrator.",
            )),
        },
        _ => View {
            tone: Tone::Attention,
            title: text("Az agent leáll", "The agent is stopping"),
            advice: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_status_has_a_plain_title_and_problems_say_what_to_do() {
        let cases = [
            (Some(r#"{"state":"connected","protocol":1}"#), Tone::Ok),
            (Some(r#"{"state":"connecting","attempt":2}"#), Tone::Working),
            (
                Some(r#"{"state":"waiting","reason":"unreachable","retry_in_ms":5000}"#),
                Tone::Attention,
            ),
            (
                Some(r#"{"state":"waiting","reason":"handshake","retry_in_ms":5000}"#),
                Tone::Problem,
            ),
            (Some(r#"{"state":"pending"}"#), Tone::Attention),
            (Some(r#"{"state":"rejected"}"#), Tone::Problem),
            (Some(r#"{"state":"unconfigured"}"#), Tone::Problem),
            (Some("not json"), Tone::Problem),
            (None, Tone::Problem),
        ];
        for (status, tone) in cases {
            for hungarian in [true, false] {
                let shown = view(status, hungarian);
                assert_eq!(shown.tone, tone, "{status:?}");
                assert!(!shown.title.is_empty() && shown.title.chars().count() < 64);
                if tone == Tone::Problem {
                    assert!(shown.advice.is_some(), "{status:?} needs a next step");
                }
            }
        }
        assert_ne!(view(None, true).title, view(None, false).title);
    }
}
