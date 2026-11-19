//! Monitor layout, frame composition and desktop-generation bookkeeping for remote capture.
//! Platform code fills monitor frames; this module decides what the viewer sees, maps viewer
//! coordinates back to the desktop and keeps queued input bound to the desktop it was meant for.

use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    fn right(self) -> i64 {
        i64::from(self.x) + i64::from(self.width)
    }

    fn bottom(self) -> i64 {
        i64::from(self.y) + i64::from(self.height)
    }

    fn contains(self, x: i64, y: i64) -> bool {
        x >= i64::from(self.x) && x < self.right() && y >= i64::from(self.y) && y < self.bottom()
    }
}

/// How the output is rotated relative to its scan-out image, as Windows reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rotation {
    Identity,
    Rotate90,
    Rotate180,
    Rotate270,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Monitor {
    /// Stable for the lifetime of the display configuration, for example `\\.\DISPLAY1`.
    pub id: String,
    /// Area on the virtual desktop, in desktop pixels.
    pub bounds: Rect,
    pub primary: bool,
    pub rotation: Rotation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Selection {
    Monitor(String),
    /// Every attached monitor, laid out as on the virtual desktop.
    Combined,
}

/// One frame in BGRA byte order, without row padding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

impl Frame {
    pub fn blank(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            bgra: vec![0; width as usize * height as usize * 4],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureError {
    /// The selection names no attached monitor, or the layout is empty or too large.
    Selection,
    /// A monitor frame does not match the monitor's size.
    Frame,
}

/// Largest composed frame accepted, so a misreported layout cannot allocate without bound.
pub const MAX_VIEW_PIXELS: u64 = 16_384 * 16_384;

/// The desktop area the viewer sees for this selection.
pub fn view(monitors: &[Monitor], selection: &Selection) -> Result<Rect, CaptureError> {
    let area = match selection {
        Selection::Monitor(id) => monitors
            .iter()
            .find(|monitor| &monitor.id == id)
            .map(|monitor| monitor.bounds)
            .ok_or(CaptureError::Selection)?,
        Selection::Combined => {
            let first = monitors.first().ok_or(CaptureError::Selection)?.bounds;
            let (mut left, mut top, mut right, mut bottom) = (
                i64::from(first.x),
                i64::from(first.y),
                first.right(),
                first.bottom(),
            );
            for monitor in &monitors[1..] {
                left = left.min(i64::from(monitor.bounds.x));
                top = top.min(i64::from(monitor.bounds.y));
                right = right.max(monitor.bounds.right());
                bottom = bottom.max(monitor.bounds.bottom());
            }
            Rect {
                x: i32::try_from(left).map_err(|_| CaptureError::Selection)?,
                y: i32::try_from(top).map_err(|_| CaptureError::Selection)?,
                width: u32::try_from(right - left).map_err(|_| CaptureError::Selection)?,
                height: u32::try_from(bottom - top).map_err(|_| CaptureError::Selection)?,
            }
        }
    };
    let pixels = u64::from(area.width) * u64::from(area.height);
    if pixels == 0 || pixels > MAX_VIEW_PIXELS {
        return Err(CaptureError::Selection);
    }
    Ok(area)
}

/// The monitors that contribute to the view.
pub fn sources<'a>(monitors: &'a [Monitor], selection: &Selection) -> Vec<&'a Monitor> {
    monitors
        .iter()
        .filter(|monitor| match selection {
            Selection::Monitor(id) => &monitor.id == id,
            Selection::Combined => true,
        })
        .collect()
}

/// Copies a monitor's scan-out frame into the view, undoing the output rotation. Areas no monitor
/// covers stay as they are, black in a fresh frame.
pub fn compose(
    view: Rect,
    monitor: &Monitor,
    source: &Frame,
    target: &mut Frame,
) -> Result<(), CaptureError> {
    let (bounds, rotation) = (monitor.bounds, monitor.rotation);
    let swapped = matches!(rotation, Rotation::Rotate90 | Rotation::Rotate270);
    let expected = if swapped {
        (bounds.height, bounds.width)
    } else {
        (bounds.width, bounds.height)
    };
    if (source.width, source.height) != expected
        || source.bgra.len() != source.width as usize * source.height as usize * 4
        || (target.width, target.height) != (view.width, view.height)
        || target.bgra.len() != target.width as usize * target.height as usize * 4
    {
        return Err(CaptureError::Frame);
    }
    let (sw, sh) = (source.width as usize, source.height as usize);
    for row in 0..bounds.height as usize {
        let desktop_y = i64::from(bounds.y) + row as i64;
        if !view.contains(i64::from(view.x), desktop_y) {
            continue;
        }
        let target_row = (desktop_y - i64::from(view.y)) as usize;
        for column in 0..bounds.width as usize {
            let desktop_x = i64::from(bounds.x) + column as i64;
            if !view.contains(desktop_x, desktop_y) {
                continue;
            }
            let (sx, sy) = match rotation {
                Rotation::Identity => (column, row),
                Rotation::Rotate90 => (row, sh - 1 - column),
                Rotation::Rotate180 => (sw - 1 - column, sh - 1 - row),
                Rotation::Rotate270 => (sw - 1 - row, column),
            };
            let from = (sy * sw + sx) * 4;
            let to =
                (target_row * target.width as usize + (desktop_x - i64::from(view.x)) as usize) * 4;
            target.bgra[to..to + 4].copy_from_slice(&source.bgra[from..from + 4]);
        }
    }
    Ok(())
}

/// Maps a pixel of the viewed frame to a desktop coordinate, only where a viewed monitor shows it.
pub fn to_desktop(
    view: Rect,
    monitors: &[Monitor],
    selection: &Selection,
    x: u32,
    y: u32,
) -> Option<(i32, i32)> {
    if x >= view.width || y >= view.height {
        return None;
    }
    let (desktop_x, desktop_y) = (
        i64::from(view.x) + i64::from(x),
        i64::from(view.y) + i64::from(y),
    );
    sources(monitors, selection)
        .iter()
        .any(|monitor| monitor.bounds.contains(desktop_x, desktop_y))
        .then(|| (i32::try_from(desktop_x).ok(), i32::try_from(desktop_y).ok()))
        .and_then(|(x, y)| x.zip(y))
}

/// Pending input for the current desktop. A desktop switch, such as the secure desktop of a UAC
/// prompt, discards everything queued for the previous one, so input never reaches a surface it
/// was not meant for. The queue is bounded.
pub struct InputQueue<T> {
    generation: u64,
    pending: VecDeque<(u64, T)>,
    limit: usize,
}

impl<T> InputQueue<T> {
    pub fn new(limit: usize) -> Self {
        Self {
            generation: 0,
            pending: VecDeque::new(),
            limit: limit.max(1),
        }
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Queues input for the desktop of `generation`; input for another desktop or beyond the
    /// limit is refused.
    pub fn push(&mut self, generation: u64, input: T) -> bool {
        if generation != self.generation || self.pending.len() >= self.limit {
            return false;
        }
        self.pending.push_back((generation, input));
        true
    }

    /// Follows the capture's desktop generation. A change, such as losing the duplicated desktop
    /// to a UAC prompt or getting it back, discards everything queued for the previous desktop and
    /// returns how many inputs were dropped.
    pub fn follow(&mut self, generation: u64) -> usize {
        if generation == self.generation {
            return 0;
        }
        self.generation = generation;
        let discarded = self.pending.len();
        self.pending.clear();
        discarded
    }

    /// The next input that still belongs to the current desktop.
    pub fn pop(&mut self) -> Option<T> {
        while let Some((generation, input)) = self.pending.pop_front() {
            if generation == self.generation {
                return Some(input);
            }
        }
        None
    }

    pub fn len(&self) -> usize {
        self.pending.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(id: &str, x: i32, y: i32, width: u32, height: u32) -> Monitor {
        Monitor {
            id: id.into(),
            bounds: Rect {
                x,
                y,
                width,
                height,
            },
            primary: x == 0 && y == 0,
            rotation: Rotation::Identity,
        }
    }

    fn filled(width: u32, height: u32, value: impl Fn(u32, u32) -> u8) -> Frame {
        let mut frame = Frame::blank(width, height);
        for y in 0..height {
            for x in 0..width {
                let at = ((y * width + x) * 4) as usize;
                frame.bgra[at..at + 4].copy_from_slice(&[value(x, y), 0, 0, 255]);
            }
        }
        frame
    }

    fn pixel(frame: &Frame, x: u32, y: u32) -> u8 {
        frame.bgra[((y * frame.width + x) * 4) as usize]
    }

    #[test]
    fn a_selected_monitor_or_the_whole_desktop_is_viewed() {
        // A secondary monitor to the left of and slightly above the primary one.
        let monitors = [
            monitor("\\\\.\\DISPLAY1", 0, 0, 4, 3),
            monitor("\\\\.\\DISPLAY2", -2, -1, 2, 2),
        ];
        assert_eq!(
            view(&monitors, &Selection::Monitor("\\\\.\\DISPLAY2".into())),
            Ok(Rect {
                x: -2,
                y: -1,
                width: 2,
                height: 2
            })
        );
        assert_eq!(
            view(&monitors, &Selection::Combined),
            Ok(Rect {
                x: -2,
                y: -1,
                width: 6,
                height: 4
            })
        );
        assert_eq!(
            view(&monitors, &Selection::Monitor("\\\\.\\DISPLAY9".into())),
            Err(CaptureError::Selection)
        );
        assert_eq!(
            view(&[], &Selection::Combined),
            Err(CaptureError::Selection)
        );
        let huge = [monitor("big", 0, 0, 20_000, 20_000)];
        assert_eq!(
            view(&huge, &Selection::Combined),
            Err(CaptureError::Selection)
        );
    }

    #[test]
    fn the_combined_view_places_each_monitor_at_its_desktop_position() {
        let monitors = [monitor("a", 0, 0, 2, 1), monitor("b", 2, 1, 1, 1)];
        let area = view(&monitors, &Selection::Combined).unwrap();
        let mut target = Frame::blank(area.width, area.height);
        compose(
            area,
            &monitors[0],
            &filled(2, 1, |x, _| 10 + x as u8),
            &mut target,
        )
        .unwrap();
        compose(area, &monitors[1], &filled(1, 1, |_, _| 99), &mut target).unwrap();
        assert_eq!(
            (
                pixel(&target, 0, 0),
                pixel(&target, 1, 0),
                pixel(&target, 2, 1)
            ),
            (10, 11, 99)
        );
        // Uncovered desktop area stays black.
        assert_eq!(pixel(&target, 0, 1), 0);
        assert_eq!(
            compose(area, &monitors[1], &filled(2, 2, |_, _| 1), &mut target),
            Err(CaptureError::Frame)
        );
    }

    #[test]
    fn rotated_outputs_are_turned_upright() {
        // A portrait monitor whose scan-out image is landscape, rotated by 90 degrees.
        let mut portrait = monitor("p", 0, 0, 2, 3);
        portrait.rotation = Rotation::Rotate90;
        let area = view(std::slice::from_ref(&portrait), &Selection::Combined).unwrap();
        let source = filled(3, 2, |x, y| (y * 3 + x) as u8);
        let mut target = Frame::blank(area.width, area.height);
        compose(area, &portrait, &source, &mut target).unwrap();
        // Desktop (column, row) reads scan-out (row, height - 1 - column).
        assert_eq!(pixel(&target, 0, 0), 3);
        assert_eq!(pixel(&target, 1, 0), 0);
        assert_eq!(pixel(&target, 0, 2), 5);
        for rotation in [Rotation::Rotate180, Rotation::Rotate270, Rotation::Identity] {
            let mut turned = monitor("t", 0, 0, 3, 2);
            turned.rotation = rotation;
            let size = if rotation == Rotation::Rotate270 {
                (2, 3)
            } else {
                (3, 2)
            };
            let source = filled(size.0, size.1, |x, y| (y * size.0 + x) as u8);
            let area = view(std::slice::from_ref(&turned), &Selection::Combined).unwrap();
            let mut target = Frame::blank(area.width, area.height);
            assert_eq!(compose(area, &turned, &source, &mut target), Ok(()));
        }
    }

    #[test]
    fn viewer_pixels_map_back_only_onto_viewed_monitors() {
        let monitors = [monitor("a", 0, 0, 4, 3), monitor("b", -2, -1, 2, 2)];
        let combined = view(&monitors, &Selection::Combined).unwrap();
        assert_eq!(
            to_desktop(combined, &monitors, &Selection::Combined, 0, 0),
            Some((-2, -1))
        );
        assert_eq!(
            to_desktop(combined, &monitors, &Selection::Combined, 2, 1),
            Some((0, 0))
        );
        // The gap below the secondary monitor belongs to no monitor.
        assert_eq!(
            to_desktop(combined, &monitors, &Selection::Combined, 0, 3),
            None
        );
        assert_eq!(
            to_desktop(combined, &monitors, &Selection::Combined, 6, 0),
            None
        );
        let single = Selection::Monitor("a".into());
        let area = view(&monitors, &single).unwrap();
        assert_eq!(to_desktop(area, &monitors, &single, 3, 2), Some((3, 2)));
    }

    #[test]
    fn a_desktop_switch_discards_input_queued_for_the_old_desktop() {
        let mut queue = InputQueue::new(3);
        let before = queue.generation();
        assert!(queue.push(before, "type password"));
        assert!(queue.push(before, "press enter"));
        assert_eq!(queue.follow(before), 0, "the same desktop keeps its input");
        assert_eq!(queue.len(), 2);
        assert_eq!(queue.follow(before + 1), 2);
        assert!(queue.is_empty());
        assert_eq!(queue.pop(), None);
        assert!(
            !queue.push(before, "late click"),
            "input meant for the previous desktop is refused"
        );
        let current = queue.generation();
        assert!(queue.push(current, "a"));
        assert!(queue.push(current, "b"));
        assert!(queue.push(current, "c"));
        assert!(!queue.push(current, "d"), "the queue is bounded");
        assert_eq!(queue.len(), 3);
        assert_eq!(queue.pop(), Some("a"));
    }
}
