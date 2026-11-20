use super::*;

const TICK: Duration = Duration::from_millis(500);

fn good(estimate: u64) -> Sample {
    Sample {
        estimate: Some(estimate),
        rtt: Some(Duration::from_millis(30)),
        loss: Some(0.0),
        jitter: Some(Duration::from_millis(5)),
        queued_frames: 0,
    }
}

/// Feeds `samples`, one per tick, and returns every change with its time.
fn run(
    controller: &mut Controller,
    start: Instant,
    samples: impl IntoIterator<Item = Sample>,
) -> Vec<(Duration, Change)> {
    samples
        .into_iter()
        .enumerate()
        .filter_map(|(index, sample)| {
            let at = TICK * (index as u32 + 1);
            controller
                .update(start + at, sample)
                .map(|change| (at, change))
        })
        .collect()
}

#[test]
fn starts_conservatively_and_raises_gradually_on_a_good_link() {
    let start = Instant::now();
    let mut control = Controller::new(Profile::Text, Limits::SOFTWARE, start);
    assert_eq!(control.current().bitrate, 1_000_000);
    let changes = run(
        &mut control,
        start,
        std::iter::repeat_n(good(8_000_000), 120),
    );
    assert!(changes.len() >= 4, "{changes:?}");
    let mut previous = (Duration::ZERO, 1_000_000);
    for (at, change) in &changes {
        assert_eq!(change.reason, Reason::Recovery);
        assert!(*at - previous.0 >= RAISE_AFTER, "too fast: {changes:?}");
        assert!(change.settings.bitrate as f64 <= previous.1 as f64 * RAISE_STEP + 1.0);
        previous = (*at, change.settings.bitrate);
    }
    let settings = control.current();
    assert_eq!(settings.bitrate, 6_000_000, "software limit");
    assert_eq!((settings.scale, settings.fps), (1.0, 30));
}

#[test]
fn sustained_loss_lowers_quality_at_once_but_a_single_spike_does_not() {
    let start = Instant::now();
    let mut control = Controller::new(Profile::Motion, Limits::HARDWARE, start);
    run(
        &mut control,
        start,
        std::iter::repeat_n(good(10_000_000), 80),
    );
    let before = control.current();
    assert!(before.bitrate > 5_000_000);

    let spike = Sample {
        loss: Some(0.12),
        ..good(10_000_000)
    };
    let mut samples = vec![spike];
    samples.extend(std::iter::repeat_n(good(10_000_000), 4));
    let begin = start + TICK * 80;
    assert!(run(&mut control, begin, samples).is_empty());

    let lossy = Sample {
        loss: Some(0.2),
        ..good(10_000_000)
    };
    let begin = start + TICK * 85;
    let changes = run(&mut control, begin, std::iter::repeat_n(lossy, 6));
    assert!(!changes.is_empty());
    assert!(changes.iter().all(|(_, c)| c.reason == Reason::Congestion));
    let (first, _) = changes[0];
    assert!(first <= TICK * 2, "reacts within two reports");
    assert!(control.current().bitrate < before.bitrate / 2);
    for pair in changes.windows(2) {
        assert!(pair[1].0 - pair[0].0 >= CUT_INTERVAL, "cuts are paced");
    }
}

#[test]
fn the_operator_sees_the_measurements_and_a_degraded_link() {
    let start = Instant::now();
    let mut control = Controller::new(Profile::Text, Limits::SOFTWARE, start);
    assert_eq!(
        control.network(),
        Network {
            rtt: None,
            loss: None,
            degraded: false
        },
        "nothing measured yet"
    );
    run(
        &mut control,
        start,
        std::iter::repeat_n(good(3_000_000), 20),
    );
    let calm = control.network();
    assert!(!calm.degraded && calm.rtt.is_some() && calm.loss.is_some());
    let lossy = Sample {
        loss: Some(0.2),
        ..good(3_000_000)
    };
    run(
        &mut control,
        start + TICK * 20,
        std::iter::repeat_n(lossy, 6),
    );
    let degraded = control.network();
    assert!(degraded.degraded);
    assert!(degraded.loss.unwrap() > 0.05);
    run(
        &mut control,
        start + TICK * 26,
        std::iter::repeat_n(good(3_000_000), 60),
    );
    assert!(
        !control.network().degraded,
        "recovers once the loss is gone"
    );
}

#[test]
fn a_fluctuating_link_does_not_make_quality_oscillate() {
    let start = Instant::now();
    let mut control = Controller::new(Profile::Text, Limits::SOFTWARE, start);
    run(
        &mut control,
        start,
        std::iter::repeat_n(good(3_000_000), 60),
    );
    let samples = (0..240).map(|index| {
        if index % 4 < 2 {
            good(3_000_000)
        } else {
            good(1_200_000)
        }
    });
    let changes = run(&mut control, start + TICK * 60, samples);
    let raises = changes
        .iter()
        .filter(|(_, c)| c.reason == Reason::Recovery)
        .count();
    assert!(raises <= 2, "{changes:?}");
    assert!(changes.len() <= 6, "{changes:?}");
}

#[test]
fn delay_and_jitter_count_as_congestion() {
    for sample in [
        Sample {
            rtt: Some(Duration::from_millis(400)),
            ..good(5_000_000)
        },
        Sample {
            jitter: Some(Duration::from_millis(200)),
            ..good(5_000_000)
        },
    ] {
        let start = Instant::now();
        let mut control = Controller::new(Profile::Text, Limits::SOFTWARE, start);
        run(
            &mut control,
            start,
            std::iter::repeat_n(good(5_000_000), 40),
        );
        let before = control.current().bitrate;
        let changes = run(
            &mut control,
            start + TICK * 40,
            std::iter::repeat_n(sample, 8),
        );
        assert!(changes.iter().any(|(_, c)| c.reason == Reason::Congestion));
        assert!(control.current().bitrate < before);
    }
}

#[test]
fn missing_measurements_never_raise_and_cap_the_bitrate() {
    let start = Instant::now();
    let mut control = Controller::new(Profile::Text, Limits::HARDWARE, start);
    run(
        &mut control,
        start,
        std::iter::repeat_n(good(10_000_000), 80),
    );
    assert!(control.current().bitrate > UNMEASURED_CAP);
    let changes = run(
        &mut control,
        start + TICK * 80,
        std::iter::repeat_n(Sample::default(), 30),
    );
    assert_eq!(changes.len(), 1, "{changes:?}");
    assert_eq!(changes[0].1.reason, Reason::Unmeasured);
    assert_eq!(control.current().bitrate, UNMEASURED_CAP);
    assert!(changes[0].0 >= STALE);

    let mut fresh = Controller::new(Profile::Motion, Limits::HARDWARE, start);
    assert!(
        run(
            &mut fresh,
            start,
            std::iter::repeat_n(Sample::default(), 40)
        )
        .is_empty()
    );
    assert_eq!(fresh.current().bitrate, START_BITRATE);
}

#[test]
fn encoder_backlog_lowers_the_frame_rate_and_recovers_it_later() {
    let start = Instant::now();
    let mut control = Controller::new(Profile::Motion, Limits::SOFTWARE, start);
    run(
        &mut control,
        start,
        std::iter::repeat_n(good(8_000_000), 80),
    );
    let before = control.current();
    let busy = Sample {
        queued_frames: 3,
        ..good(8_000_000)
    };
    let changes = run(
        &mut control,
        start + TICK * 80,
        std::iter::repeat_n(busy, 2),
    );
    assert!(changes.iter().all(|(_, c)| c.reason == Reason::Backlog));
    let slowed = control.current();
    assert!(slowed.fps < before.fps);
    assert_eq!(slowed.bitrate, before.bitrate);
    run(
        &mut control,
        start + TICK * 82,
        std::iter::repeat_n(good(8_000_000), 40),
    );
    assert_eq!(control.current().fps, before.fps);
}

#[test]
fn text_keeps_resolution_and_motion_keeps_frame_rate() {
    let start = Instant::now();
    let mut text = Controller::new(Profile::Text, Limits::HARDWARE, start);
    let mut motion = Controller::new(Profile::Motion, Limits::HARDWARE, start);
    for control in [&mut text, &mut motion] {
        run(control, start, std::iter::repeat_n(good(10_000_000), 80));
        let lossy = Sample {
            loss: Some(0.3),
            ..good(1_000_000)
        };
        run(control, start + TICK * 80, std::iter::repeat_n(lossy, 3));
        run(
            control,
            start + TICK * 83,
            std::iter::repeat_n(good(1_000_000), 3),
        );
    }
    let (text, motion) = (text.current(), motion.current());
    assert!(text.scale > motion.scale, "{text:?} {motion:?}");
    assert!(motion.fps > text.fps, "{text:?} {motion:?}");

    let mut control = Controller::new(Profile::Text, Limits::SOFTWARE, start);
    let change = control.set_profile(Profile::Motion, start).unwrap();
    assert_eq!(change.reason, Reason::Profile);
    assert!(control.set_profile(Profile::Motion, start).is_none());
}

#[test]
fn ladder_steps_need_a_margin_to_move_up() {
    let start = Instant::now();
    let mut control = Controller::new(Profile::Motion, Limits::HARDWARE, start);
    // Just above the 1.2 Mbps rung threshold, but not by the margin.
    let changes = run(
        &mut control,
        start,
        std::iter::repeat_n(good(1_500_000), 80),
    );
    assert!(
        changes.iter().all(|(_, c)| c.settings.scale == 0.5),
        "{changes:?}"
    );
    run(
        &mut control,
        start + TICK * 80,
        std::iter::repeat_n(good(2_000_000), 40),
    );
    assert_eq!(control.current().scale, 0.75);
}

#[test]
fn reconnecting_starts_over() {
    let start = Instant::now();
    let mut control = Controller::new(Profile::Text, Limits::HARDWARE, start);
    run(
        &mut control,
        start,
        std::iter::repeat_n(good(10_000_000), 80),
    );
    let change = control.reconnect(start + TICK * 81);
    assert_eq!(change.reason, Reason::Reconnect);
    assert_eq!(change.settings.bitrate, START_BITRATE);
    let early = run(
        &mut control,
        start + TICK * 81,
        std::iter::repeat_n(good(10_000_000), 4),
    );
    assert!(early.is_empty(), "no raise before a calm period: {early:?}");
}

#[test]
fn frame_size_respects_the_encoder_limit_and_even_dimensions() {
    assert_eq!(
        frame_size((1920, 1080), 1.0, Limits::SOFTWARE),
        (1920, 1080)
    );
    assert_eq!(
        frame_size((1920, 1080), 0.75, Limits::SOFTWARE),
        (1440, 810)
    );
    let (width, height) = frame_size((3840, 2160), 1.0, Limits::SOFTWARE);
    assert!(u64::from(width) * u64::from(height) <= Limits::SOFTWARE.max_pixels);
    assert_eq!((width % 2, height % 2), (0, 0));
    assert_eq!(
        frame_size((3840, 2160), 1.0, Limits::HARDWARE),
        (3840, 2160)
    );
    assert_eq!(frame_size((1366, 768), 0.375, Limits::SOFTWARE), (512, 288));
    assert_eq!(frame_size((200, 100), 0.1, Limits::SOFTWARE), (160, 90));
}
