//! Monitor choice, work areas and fullscreen checks (pure, so they're tested on every OS).

/// A monitor in physical root-window pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Monitor {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub primary: bool,
}

impl Monitor {
    fn right(&self) -> i32 {
        self.x + self.width as i32
    }

    fn bottom(&self) -> i32 {
        self.y + self.height as i32
    }

    fn contains(&self, (x, y): (i32, i32)) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }
}

/// The monitor holding `point`.
pub fn monitor_at(monitors: &[Monitor], point: (i32, i32)) -> Option<usize> {
    monitors.iter().position(|monitor| monitor.contains(point))
}

/// The pointer's monitor; else the one the notch is on; else the primary (or first) one.
pub fn pick_monitor(
    monitors: &[Monitor],
    pointer: Option<(i32, i32)>,
    current: Option<usize>,
) -> Option<usize> {
    pointer
        .and_then(|point| monitor_at(monitors, point))
        .or(current.filter(|&index| index < monitors.len()))
        .or_else(|| monitors.iter().position(|monitor| monitor.primary))
        .or((!monitors.is_empty()).then_some(0))
}

/// True when the window `(x, y, width, height)` covers the whole monitor.
pub fn covers_monitor(window: (i32, i32, u32, u32), monitor: &Monitor) -> bool {
    let (x, y, width, height) = window;
    x <= monitor.x
        && y <= monitor.y
        && x + width as i32 >= monitor.right()
        && y + height as i32 >= monitor.bottom()
}

/// The monitor's usable area as `[left, top, right, bottom]`: the desktop work area
/// (`_NET_WORKAREA`, also `[left, top, right, bottom]` here) clipped to the monitor, or the
/// whole monitor when there is none or it doesn't overlap.
pub fn work_area(monitor: &Monitor, desktop: Option<[i32; 4]>) -> [i32; 4] {
    let whole = [monitor.x, monitor.y, monitor.right(), monitor.bottom()];
    let Some([left, top, right, bottom]) = desktop else {
        return whole;
    };
    let clipped = [
        left.max(whole[0]),
        top.max(whole[1]),
        right.min(whole[2]),
        bottom.min(whole[3]),
    ];
    if clipped[0] < clipped[2] && clipped[1] < clipped[3] {
        clipped
    } else {
        whole
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two() -> Vec<Monitor> {
        vec![
            Monitor {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
                primary: false,
            },
            Monitor {
                x: 1920,
                y: 0,
                width: 1920,
                height: 1080,
                primary: true,
            },
        ]
    }

    #[test]
    fn pointer_picks_its_monitor() {
        assert_eq!(pick_monitor(&two(), Some((2000, 10)), Some(0)), Some(1));
        assert_eq!(pick_monitor(&two(), Some((5, 5)), Some(1)), Some(0));
    }

    #[test]
    fn no_pointer_keeps_current() {
        assert_eq!(pick_monitor(&two(), None, Some(0)), Some(0));
    }

    #[test]
    fn no_pointer_no_current_uses_primary() {
        assert_eq!(pick_monitor(&two(), None, None), Some(1));
        assert_eq!(pick_monitor(&[], None, None), None);
    }

    #[test]
    fn pointer_off_every_monitor_falls_back() {
        assert_eq!(pick_monitor(&two(), Some((-50, -50)), Some(0)), Some(0));
    }

    #[test]
    fn fullscreen_must_cover_the_monitor() {
        let monitor = two()[1];
        assert!(covers_monitor((1920, 0, 1920, 1080), &monitor));
        assert!(!covers_monitor((1920, 0, 1920, 1070), &monitor));
        assert!(!covers_monitor((0, 0, 1920, 1080), &monitor));
    }

    #[test]
    fn work_area_is_clipped_to_the_monitor() {
        let monitor = two()[1];
        // A panel along the top of the whole desktop (both monitors), 32 px tall.
        assert_eq!(
            work_area(&monitor, Some([0, 32, 3840, 1080])),
            [1920, 32, 3840, 1080]
        );
        assert_eq!(work_area(&monitor, None), [1920, 0, 3840, 1080]);
        // A work area that misses the monitor entirely is ignored.
        assert_eq!(
            work_area(&monitor, Some([0, 0, 100, 100])),
            [1920, 0, 3840, 1080]
        );
    }
}
