//! H.264 codec negotiation: picks the browser's offered H.264 format and the local encoder that
//! serves it. Hardware encoders win over software ones; constrained baseline wins over other
//! profiles, because every browser decodes it and it has no frame reordering delay.

use super::adapt::Limits;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Profile {
    ConstrainedBaseline,
    Baseline,
    Main,
    High,
}

/// One H.264 format the browser offered, as its SDP describes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Offer {
    pub payload_type: u8,
    pub profile_level_id: u32,
    pub packetization_mode: u8,
}

impl Offer {
    /// The profile per RFC 6184 section 8.1, from `profile_idc` and the constraint flags.
    pub fn profile(&self) -> Option<Profile> {
        let idc = (self.profile_level_id >> 16) & 0xff;
        let constraints = (self.profile_level_id >> 8) & 0xff;
        let set0 = constraints & 0x80 != 0;
        let set1 = constraints & 0x40 != 0;
        match idc {
            0x42 if set1 => Some(Profile::ConstrainedBaseline),
            0x4d if set0 => Some(Profile::ConstrainedBaseline),
            0x42 => Some(Profile::Baseline),
            0x4d => Some(Profile::Main),
            0x64 => Some(Profile::High),
            _ => None,
        }
    }

    pub fn level(&self) -> u8 {
        (self.profile_level_id & 0xff) as u8
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Hardware,
    Software,
}

/// A local H.264 encoder the host found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Encoder {
    pub name: String,
    pub kind: Kind,
    pub profiles: Vec<Profile>,
    pub limits: Limits,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub offer: Offer,
    pub profile: Profile,
    /// Index into the encoder list.
    pub encoder: usize,
}

/// The best pairing, or `None` when nothing offered can be sent. Only packetization mode 1 is
/// used, so frames larger than one packet are fragmented rather than refused.
pub fn choose(offers: &[Offer], encoders: &[Encoder]) -> Option<Choice> {
    let mut best: Option<(Choice, (bool, Profile, usize))> = None;
    for offer in offers.iter().filter(|offer| offer.packetization_mode == 1) {
        let Some(profile) = offer.profile() else {
            continue;
        };
        for (index, encoder) in encoders.iter().enumerate() {
            if !encoder.profiles.contains(&profile) {
                continue;
            }
            // Lower ranks win: hardware first, then the simplest profile, then the earlier offer.
            let rank = (encoder.kind != Kind::Hardware, profile, index);
            if best.as_ref().is_none_or(|(_, current)| rank < *current) {
                best = Some((
                    Choice {
                        offer: *offer,
                        profile,
                        encoder: index,
                    },
                    rank,
                ));
            }
        }
    }
    best.map(|(choice, _)| choice)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offer(payload_type: u8, profile_level_id: u32, packetization_mode: u8) -> Offer {
        Offer {
            payload_type,
            profile_level_id,
            packetization_mode,
        }
    }

    fn encoder(kind: Kind, profiles: &[Profile]) -> Encoder {
        Encoder {
            name: format!("{kind:?}"),
            kind,
            profiles: profiles.to_vec(),
            limits: match kind {
                Kind::Hardware => Limits::HARDWARE,
                Kind::Software => Limits::SOFTWARE,
            },
        }
    }

    #[test]
    fn profiles_follow_rfc_6184() {
        assert_eq!(
            offer(0, 0x42e01f, 1).profile(),
            Some(Profile::ConstrainedBaseline)
        );
        assert_eq!(
            offer(0, 0x4de01f, 1).profile(),
            Some(Profile::ConstrainedBaseline)
        );
        assert_eq!(offer(0, 0x42001f, 1).profile(), Some(Profile::Baseline));
        assert_eq!(offer(0, 0x4d001f, 1).profile(), Some(Profile::Main));
        assert_eq!(offer(0, 0x64001f, 1).profile(), Some(Profile::High));
        assert_eq!(offer(0, 0xf4001f, 1).profile(), None);
        assert_eq!(offer(0, 0x42e01f, 1).level(), 0x1f);
    }

    #[test]
    fn hardware_and_constrained_baseline_win_with_packetization_mode_one() {
        let browser = [
            offer(102, 0x42001f, 1),
            offer(104, 0x42e01f, 0),
            offer(106, 0x42e01f, 1),
            offer(112, 0x64001f, 1),
        ];
        let software = encoder(
            Kind::Software,
            &[Profile::ConstrainedBaseline, Profile::Baseline],
        );
        let hardware = encoder(
            Kind::Hardware,
            &[Profile::ConstrainedBaseline, Profile::High],
        );
        let choice = choose(&browser, &[software.clone(), hardware.clone()]).unwrap();
        assert_eq!((choice.offer.payload_type, choice.encoder), (106, 1));
        assert_eq!(choice.profile, Profile::ConstrainedBaseline);
        let choice = choose(&browser, std::slice::from_ref(&software)).unwrap();
        assert_eq!((choice.offer.payload_type, choice.encoder), (106, 0));
        let high_only = encoder(Kind::Hardware, &[Profile::High]);
        let choice = choose(&browser, &[software, high_only]).unwrap();
        assert_eq!(
            choice.encoder, 1,
            "hardware first, even with another profile"
        );
        assert_eq!(choice.offer.payload_type, 112);
    }

    #[test]
    fn nothing_is_chosen_without_a_usable_pair() {
        let software = encoder(Kind::Software, &[Profile::ConstrainedBaseline]);
        assert_eq!(choose(&[], std::slice::from_ref(&software)), None);
        assert_eq!(
            choose(&[offer(96, 0x42e01f, 0)], std::slice::from_ref(&software)),
            None
        );
        assert_eq!(
            choose(&[offer(96, 0x64001f, 1)], std::slice::from_ref(&software)),
            None
        );
        assert_eq!(choose(&[offer(96, 0x42e01f, 1)], &[]), None);
    }
}
