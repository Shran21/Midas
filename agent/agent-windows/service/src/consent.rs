//! The question the service sends to the consent window in the user's session, the answer that
//! comes back, and the window's texts. The service keeps its own deadline; the window only shows it.

use midas_remote_auth::Transfer;
use uuid::Uuid;

const QUESTION: &[u8; 4] = b"MCQ1";
const ANSWER: &[u8; 4] = b"MCA1";
/// The agent already limits names to 200 characters; four bytes each in UTF-8.
const NAME_BYTES: usize = 800;
pub const LONGEST_WAIT: u32 = 300;
/// The service's word that the allowed session runs: the window turns into its indicator.
pub const INDICATE: &[u8] = b"indicate/1";
/// The local user pressed End on the indicator.
pub const END: &[u8] = b"end/1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Question {
    pub request: Uuid,
    pub operator: String,
    pub customer: String,
    pub administrative: bool,
    pub uac: bool,
    pub transfer: Transfer,
    pub seconds: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    Allow,
    Decline,
    Expired,
}

fn take<'a>(bytes: &mut &'a [u8], count: usize) -> Option<&'a [u8]> {
    let (head, rest) = bytes.split_at_checked(count)?;
    *bytes = rest;
    Some(head)
}

fn text(bytes: &mut &[u8]) -> Option<String> {
    let length = u16::from_le_bytes(take(bytes, 2)?.try_into().ok()?) as usize;
    if length == 0 || length > NAME_BYTES {
        return None;
    }
    String::from_utf8(take(bytes, length)?.to_vec()).ok()
}

impl Question {
    pub fn encode(&self) -> Option<Vec<u8>> {
        let mut bytes = QUESTION.to_vec();
        bytes.extend_from_slice(self.request.as_bytes());
        bytes.extend_from_slice(&self.seconds.to_le_bytes());
        bytes.push(
            u8::from(self.administrative)
                | (u8::from(self.uac) << 1)
                | (u8::from(self.transfer.clipboard) << 2)
                | (u8::from(self.transfer.upload) << 3)
                | (u8::from(self.transfer.download) << 4),
        );
        for name in [&self.operator, &self.customer] {
            if name.is_empty() || name.len() > NAME_BYTES {
                return None;
            }
            bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
            bytes.extend_from_slice(name.as_bytes());
        }
        Some(bytes)
    }

    pub fn decode(mut bytes: &[u8]) -> Option<Self> {
        if take(&mut bytes, 4)? != QUESTION {
            return None;
        }
        let request = Uuid::from_slice(take(&mut bytes, 16)?).ok()?;
        let seconds = u32::from_le_bytes(take(&mut bytes, 4)?.try_into().ok()?);
        let flags = take(&mut bytes, 1)?[0];
        let question = Self {
            request,
            operator: text(&mut bytes)?,
            customer: text(&mut bytes)?,
            administrative: flags & 1 != 0,
            uac: flags & 2 != 0,
            transfer: Transfer {
                clipboard: flags & 4 != 0,
                upload: flags & 8 != 0,
                download: flags & 16 != 0,
            },
            seconds,
        };
        (bytes.is_empty()
            && flags < 32
            && !request.is_nil()
            && (1..=LONGEST_WAIT).contains(&seconds))
        .then_some(question)
    }
}

pub fn encode_answer(request: Uuid, answer: Answer) -> Vec<u8> {
    let mut bytes = ANSWER.to_vec();
    bytes.extend_from_slice(request.as_bytes());
    bytes.push(match answer {
        Answer::Allow => 1,
        Answer::Decline => 2,
        Answer::Expired => 3,
    });
    bytes
}

pub fn decode_answer(mut bytes: &[u8]) -> Option<(Uuid, Answer)> {
    if take(&mut bytes, 4)? != ANSWER {
        return None;
    }
    let request = Uuid::from_slice(take(&mut bytes, 16)?).ok()?;
    let answer = match take(&mut bytes, 1)? {
        [1] => Answer::Allow,
        [2] => Answer::Decline,
        [3] => Answer::Expired,
        _ => return None,
    };
    bytes.is_empty().then_some((request, answer))
}

/// What the consent window shows, in the user's language.
#[derive(Debug, PartialEq, Eq)]
pub struct Texts {
    pub title: String,
    pub instruction: String,
    pub content: String,
    pub allow: String,
    pub decline: String,
}

pub fn texts(question: &Question, hungarian: bool) -> Texts {
    let mut lines = Vec::new();
    if hungarian {
        lines.push(format!("Szervezet: {}", question.customer));
        lines.push(format!(
            "Mód: {}",
            if question.administrative {
                "rendszergazdai távvezérlés"
            } else {
                "normál távvezérlés"
            }
        ));
        lines.push(String::new());
        lines.push("Ha engedélyezed, a technikus:".into());
        lines.push("• látja a képernyődet".into());
        lines.push("• használhatja az egeret és a billentyűzetet".into());
        lines.push("• a programokat a te jogosultságoddal indítja".into());
        if question.uac {
            lines.push("• kezelheti a Windows rendszergazdai engedélykéréseit (UAC)".into());
        }
        if question.administrative {
            lines.push("• a cége által engedélyezett rendszergazdai műveleteket futtathat".into());
        }
        if question.transfer.clipboard {
            lines.push("• szöveget másolhat a vágólapodra és onnan".into());
        }
        if question.transfer.upload {
            lines.push("• fájlokat küldhet a gépedre, a Letöltések\\MIDAS mappába".into());
        }
        if question.transfer.download {
            lines.push("• letöltheti azokat a fájlokat, amelyekhez neked hozzáférésed van".into());
        }
    } else {
        lines.push(format!("Organisation: {}", question.customer));
        lines.push(format!(
            "Mode: {}",
            if question.administrative {
                "administrative remote control"
            } else {
                "normal remote control"
            }
        ));
        lines.push(String::new());
        lines.push("If you allow it, the technician can:".into());
        lines.push("• see your screen".into());
        lines.push("• use the mouse and keyboard".into());
        lines.push("• start programs with your permissions".into());
        if question.uac {
            lines.push("• handle Windows administrator permission prompts (UAC)".into());
        }
        if question.administrative {
            lines.push("• run administrative actions approved by their company".into());
        }
        if question.transfer.clipboard {
            lines.push("• copy text to and from your clipboard".into());
        }
        if question.transfer.upload {
            lines.push("• send files to your computer, into Downloads\\MIDAS".into());
        }
        if question.transfer.download {
            lines.push("• download the files you can open".into());
        }
    }
    Texts {
        title: if hungarian {
            "MIDAS távsegítés"
        } else {
            "MIDAS remote support"
        }
        .into(),
        instruction: if hungarian {
            format!("{} kapcsolódni szeretne a gépedhez", question.operator)
        } else {
            format!("{} wants to connect to your computer", question.operator)
        },
        content: lines.join("\n"),
        allow: if hungarian { "Engedélyezés" } else { "Allow" }.into(),
        decline: if hungarian { "Elutasítás" } else { "Decline" }.into(),
    }
}

/// What the session indicator shows while the technician is connected.
#[derive(Debug, PartialEq, Eq)]
pub struct Indicator {
    pub title: String,
    pub line: String,
    pub end: String,
}

pub fn indicator(question: &Question, hungarian: bool) -> Indicator {
    let mode = match (question.administrative, hungarian) {
        (true, true) => " · rendszergazdai mód",
        (true, false) => " · administrative mode",
        (false, _) => "",
    };
    if hungarian {
        Indicator {
            title: "MIDAS távsegítés".into(),
            line: format!(
                "{} ({}) látja és vezérli a gépedet{mode}.",
                question.operator, question.customer
            ),
            end: "Befejezés".into(),
        }
    } else {
        Indicator {
            title: "MIDAS remote support".into(),
            line: format!(
                "{} ({}) can see and control your computer{mode}.",
                question.operator, question.customer
            ),
            end: "End".into(),
        }
    }
}

/// The countdown line; it is not a live region, so a screen reader does not repeat it.
pub fn remaining(seconds: u64, hungarian: bool) -> String {
    if hungarian {
        format!("A kérés {seconds} másodperc múlva lejár. Lejáratkor nem jön létre kapcsolat.")
    } else {
        format!("The request expires in {seconds} seconds. No connection is made when it expires.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn question() -> Question {
        Question {
            request: Uuid::new_v4(),
            operator: "Kovács Béla".into(),
            customer: "Norda Kft.".into(),
            administrative: true,
            uac: false,
            transfer: Transfer::NONE,
            seconds: 60,
        }
    }

    #[test]
    fn messages_round_trip_and_reject_anything_else() {
        let question = question();
        let bytes = question.encode().unwrap();
        assert_eq!(Question::decode(&bytes), Some(question.clone()));
        for cut in 0..bytes.len() {
            assert_eq!(Question::decode(&bytes[..cut]), None);
        }
        let mut longer = bytes.clone();
        longer.push(0);
        assert_eq!(Question::decode(&longer), None);
        let everything = Question {
            uac: true,
            transfer: Transfer {
                clipboard: true,
                upload: true,
                download: true,
            },
            ..question.clone()
        };
        assert_eq!(
            Question::decode(&everything.encode().unwrap()),
            Some(everything)
        );
        let mut unknown = bytes.clone();
        unknown[24] |= 32;
        assert_eq!(Question::decode(&unknown), None);
        for seconds in [0, LONGEST_WAIT + 1] {
            let encoded = Question {
                seconds,
                ..question.clone()
            }
            .encode()
            .unwrap();
            assert_eq!(Question::decode(&encoded), None);
        }
        assert_eq!(
            Question {
                operator: String::new(),
                ..question.clone()
            }
            .encode(),
            None
        );
        let answer = encode_answer(question.request, Answer::Allow);
        assert_eq!(
            decode_answer(&answer),
            Some((question.request, Answer::Allow))
        );
        let mut other = answer.clone();
        other[20] = 4;
        assert_eq!(decode_answer(&other), None);
        assert_eq!(decode_answer(&bytes), None);
        assert_eq!(Question::decode(&answer), None);
    }

    #[test]
    fn the_window_names_the_technician_the_customer_and_each_capability() {
        let normal = Question {
            administrative: false,
            ..question()
        };
        for hungarian in [true, false] {
            let plain = texts(&normal, hungarian);
            assert!(plain.instruction.starts_with("Kovács Béla"));
            assert!(plain.content.contains("Norda Kft."));
            assert!(!plain.content.contains("UAC"));
            let full = texts(
                &Question {
                    uac: true,
                    ..question()
                },
                hungarian,
            );
            assert!(full.content.contains("UAC"));
            assert_eq!(
                full.content.lines().count(),
                plain.content.lines().count() + 2
            );
        }
        for hungarian in [true, false] {
            let plain = texts(&question(), hungarian).content.lines().count();
            for (transfer, word) in [
                (
                    Transfer {
                        clipboard: true,
                        ..Transfer::NONE
                    },
                    if hungarian { "vágólap" } else { "clipboard" },
                ),
                (
                    Transfer {
                        upload: true,
                        ..Transfer::NONE
                    },
                    "MIDAS",
                ),
                (
                    Transfer {
                        download: true,
                        ..Transfer::NONE
                    },
                    if hungarian { "letöltheti" } else { "download" },
                ),
            ] {
                let shown = texts(
                    &Question {
                        transfer,
                        ..question()
                    },
                    hungarian,
                );
                assert!(shown.content.contains(word), "{word}");
                assert_eq!(shown.content.lines().count(), plain + 1);
            }
        }
        assert_eq!(texts(&normal, true).allow, "Engedélyezés");
        assert_eq!(texts(&normal, false).decline, "Decline");
        assert!(remaining(42, true).contains("42"));
    }
}
