//! When the notch shows itself: a pure state machine driven by timestamped events.

use std::time::{Duration, Instant};

/// Cursor must rest this long on the sliver before it pops out.
pub const HOVER_DELAY: Duration = Duration::from_millis(150);
/// Grace period after the cursor leaves, so edge jitter does not flicker.
pub const LEAVE_DELAY: Duration = Duration::from_millis(500);
/// How long an unprompted peek stays out.
pub const PEEK_FOR: Duration = Duration::from_secs(4);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Hidden,
    Sliver,
    Peek,
    Hover,
    Open,
}

/// The window shape to animate toward.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    Sliver,
    Compact,
    Expanded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    CursorEntered,
    CursorLeft,
    Tick,
    Click,
    Shortcut,
    Dismiss,
    FocusLost,
    Peek(String),
    /// A reply is streaming (true) or finished (false).
    Busy(bool),
    /// The reply was stopped or abandoned: unpin without an "All done" peek.
    Cancelled,
    Fullscreen(bool),
}

#[derive(Debug, Clone)]
pub struct Presence {
    state: State,
    inside: bool,
    busy: bool,
    hover_at: Option<Instant>,
    leave_at: Option<Instant>,
    peek_until: Option<Instant>,
    message: Option<String>,
    queued: Option<String>,
}

impl Default for Presence {
    fn default() -> Self {
        Self::new()
    }
}

impl Presence {
    pub fn new() -> Self {
        Self {
            state: State::Sliver,
            inside: false,
            busy: false,
            hover_at: None,
            leave_at: None,
            peek_until: None,
            message: None,
            queued: None,
        }
    }

    pub fn state(&self) -> State {
        self.state
    }

    pub fn shape(&self) -> Shape {
        match self.state {
            State::Hidden | State::Sliver => Shape::Sliver,
            State::Peek | State::Hover => Shape::Compact,
            State::Open => Shape::Expanded,
        }
    }

    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    /// The earliest moment a `Tick` could change something.
    pub fn next_deadline(&self) -> Option<Instant> {
        [self.hover_at, self.leave_at, self.peek_until]
            .into_iter()
            .flatten()
            .min()
    }

    /// Applies `event` at `now`; returns whether the state or message changed.
    pub fn on(&mut self, event: Event, now: Instant) -> bool {
        let before = (self.state, self.message.clone());
        match event {
            Event::CursorEntered => {
                self.inside = true;
                self.leave_at = None;
                if self.state == State::Sliver {
                    self.hover_at = Some(now + HOVER_DELAY);
                }
            }
            Event::CursorLeft => {
                self.inside = false;
                self.hover_at = None;
                if matches!(self.state, State::Hover | State::Peek) {
                    self.leave_at = Some(now + LEAVE_DELAY);
                }
            }
            Event::Tick => {}
            Event::Cancelled => self.busy = false,
            Event::Click => {
                if matches!(self.state, State::Hover | State::Peek) {
                    self.open();
                }
            }
            Event::Shortcut => {
                if self.state == State::Open {
                    self.close();
                } else {
                    self.open();
                }
            }
            Event::Dismiss | Event::FocusLost => {
                if self.state == State::Open {
                    self.close();
                }
            }
            Event::Peek(message) => self.peek(message, now),
            Event::Busy(busy) => {
                self.busy = busy;
                if !busy && self.state != State::Open {
                    self.peek("All done".into(), now);
                }
            }
            Event::Fullscreen(true) => {
                if !matches!(self.state, State::Open | State::Hidden) {
                    self.state = State::Hidden;
                    self.inside = false;
                    self.clear_deadlines();
                    self.message = None;
                }
            }
            Event::Fullscreen(false) => {
                if self.state == State::Hidden {
                    self.state = State::Sliver;
                    if let Some(message) = self.queued.take() {
                        self.peek(message, now);
                    }
                }
            }
        }
        self.advance(now);
        (self.state, self.message.clone()) != before
    }

    fn peek(&mut self, message: String, now: Instant) {
        match self.state {
            State::Hidden => self.queued = Some(message),
            State::Open => self.message = Some(message),
            State::Sliver | State::Hover | State::Peek => {
                self.state = State::Peek;
                self.message = Some(message);
                self.hover_at = None;
                self.peek_until = Some(now + PEEK_FOR);
            }
        }
    }

    fn open(&mut self) {
        self.state = State::Open;
        self.message = None;
        self.clear_deadlines();
    }

    fn close(&mut self) {
        self.clear_deadlines();
        self.message = None;
        // A reply still streaming keeps the pill out so the mascot can be seen talking.
        self.state = if self.busy {
            State::Peek
        } else {
            State::Sliver
        };
    }

    fn clear_deadlines(&mut self) {
        self.hover_at = None;
        self.leave_at = None;
        self.peek_until = None;
    }

    fn advance(&mut self, now: Instant) {
        if self.hover_at.is_some_and(|at| now >= at) {
            self.hover_at = None;
            if self.state == State::Sliver && self.inside {
                self.state = State::Hover;
            }
        }
        if self.state == State::Peek
            && !self.busy
            && self.peek_until.is_none_or(|until| now >= until)
        {
            self.peek_until = None;
            self.message = None;
            self.state = if self.inside {
                State::Hover
            } else {
                State::Sliver
            };
        }
        if self.leave_at.is_some_and(|at| now >= at) {
            self.leave_at = None;
            if self.state == State::Hover && !self.inside {
                self.state = State::Sliver;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::OnceLock;

    fn at(ms: u64) -> Instant {
        static BASE: OnceLock<Instant> = OnceLock::new();
        *BASE.get_or_init(Instant::now) + Duration::from_millis(ms)
    }

    fn hovering() -> Presence {
        let mut p = Presence::new();
        p.on(Event::CursorEntered, at(0));
        p.on(Event::Tick, at(150));
        assert_eq!(p.state(), State::Hover);
        p
    }

    #[test]
    fn brushing_past_does_not_open() {
        let mut p = Presence::new();
        p.on(Event::CursorEntered, at(0));
        p.on(Event::CursorLeft, at(100));
        p.on(Event::Tick, at(200));
        assert_eq!(p.state(), State::Sliver);
    }

    #[test]
    fn resting_opens_hover() {
        let mut p = Presence::new();
        p.on(Event::CursorEntered, at(0));
        p.on(Event::Tick, at(149));
        assert_eq!(p.state(), State::Sliver);
        assert!(p.on(Event::Tick, at(150)));
        assert_eq!(p.state(), State::Hover);
        assert_eq!(p.shape(), Shape::Compact);
    }

    #[test]
    fn leaving_hover_shrinks_after_delay() {
        let mut p = hovering();
        p.on(Event::CursorLeft, at(1000));
        p.on(Event::Tick, at(1400));
        assert_eq!(p.state(), State::Hover);
        p.on(Event::Tick, at(1500));
        assert_eq!(p.state(), State::Sliver);
        assert_eq!(p.shape(), Shape::Sliver);
    }

    #[test]
    fn edge_jitter_keeps_hover() {
        let mut p = hovering();
        p.on(Event::CursorLeft, at(1000));
        p.on(Event::CursorEntered, at(1200));
        p.on(Event::Tick, at(1800));
        assert_eq!(p.state(), State::Hover);
    }

    #[test]
    fn peek_expires_to_sliver() {
        let mut p = Presence::new();
        p.on(Event::Peek("hi".into()), at(0));
        assert_eq!(p.state(), State::Peek);
        assert_eq!(p.message(), Some("hi"));
        assert_eq!(p.shape(), Shape::Compact);
        p.on(Event::Tick, at(3999));
        assert_eq!(p.state(), State::Peek);
        p.on(Event::Tick, at(4000));
        assert_eq!(p.state(), State::Sliver);
        assert_eq!(p.message(), None);
    }

    #[test]
    fn peek_with_cursor_inside_becomes_hover() {
        let mut p = Presence::new();
        p.on(Event::Peek("hi".into()), at(0));
        p.on(Event::CursorEntered, at(100));
        p.on(Event::Tick, at(4000));
        assert_eq!(p.state(), State::Hover);
        assert_eq!(p.message(), None);
    }

    #[test]
    fn latest_peek_wins() {
        let mut p = Presence::new();
        p.on(Event::Peek("a".into()), at(0));
        p.on(Event::Peek("b".into()), at(3000));
        p.on(Event::Tick, at(5000));
        assert_eq!(p.state(), State::Peek);
        assert_eq!(p.message(), Some("b"));
    }

    #[test]
    fn peek_while_hovering_keeps_open() {
        let mut p = hovering();
        p.on(Event::Peek("x".into()), at(1000));
        assert_eq!(p.message(), Some("x"));
        p.on(Event::Tick, at(5000));
        assert_eq!(p.state(), State::Hover);
    }

    #[test]
    fn click_and_shortcut_open() {
        let mut p = hovering();
        p.on(Event::Click, at(200));
        assert_eq!(p.state(), State::Open);
        assert_eq!(p.shape(), Shape::Expanded);
        p.on(Event::Shortcut, at(300));
        assert_eq!(p.state(), State::Sliver);
        p.on(Event::Shortcut, at(400));
        assert_eq!(p.state(), State::Open);
        p.on(Event::Dismiss, at(500));
        assert_eq!(p.state(), State::Sliver);
    }

    #[test]
    fn click_on_sliver_does_nothing() {
        let mut p = Presence::new();
        p.on(Event::Click, at(0));
        assert_eq!(p.state(), State::Sliver);
    }

    #[test]
    fn fullscreen_hides_and_replays_peek() {
        let mut p = hovering();
        p.on(Event::Fullscreen(true), at(200));
        assert_eq!(p.state(), State::Hidden);
        assert_eq!(p.shape(), Shape::Sliver);
        p.on(Event::Peek("t".into()), at(300));
        assert_eq!(p.state(), State::Hidden);
        p.on(Event::Fullscreen(false), at(10_000));
        assert_eq!(p.state(), State::Peek);
        assert_eq!(p.message(), Some("t"));
        p.on(Event::Tick, at(14_000));
        assert_eq!(p.state(), State::Sliver);
    }

    #[test]
    fn fullscreen_does_not_close_open_panel() {
        let mut p = hovering();
        p.on(Event::Click, at(200));
        p.on(Event::Fullscreen(true), at(300));
        assert_eq!(p.state(), State::Open);
    }

    #[test]
    fn shortcut_opens_during_fullscreen() {
        let mut p = Presence::new();
        p.on(Event::Fullscreen(true), at(0));
        p.on(Event::Shortcut, at(100));
        assert_eq!(p.state(), State::Open);
    }

    #[test]
    fn focus_loss_while_busy_pins_peek() {
        let mut p = Presence::new();
        p.on(Event::Shortcut, at(0));
        p.on(Event::Busy(true), at(0));
        p.on(Event::FocusLost, at(0));
        assert_eq!(p.state(), State::Peek);
        p.on(Event::Tick, at(60_000));
        assert_eq!(p.state(), State::Peek);
        p.on(Event::Busy(false), at(60_000));
        assert_eq!(p.message(), Some("All done"));
        p.on(Event::Tick, at(63_999));
        assert_eq!(p.state(), State::Peek);
        p.on(Event::Tick, at(64_000));
        assert_eq!(p.state(), State::Sliver);
    }

    #[test]
    fn busy_finishing_while_open_stays_open() {
        let mut p = Presence::new();
        p.on(Event::Shortcut, at(0));
        p.on(Event::Busy(true), at(0));
        p.on(Event::Busy(false), at(1000));
        assert_eq!(p.state(), State::Open);
    }

    #[test]
    fn peek_while_open_only_sets_message() {
        let mut p = Presence::new();
        p.on(Event::Shortcut, at(0));
        p.on(Event::Peek("hey".into()), at(10));
        assert_eq!(p.state(), State::Open);
        assert_eq!(p.message(), Some("hey"));
    }

    #[test]
    fn cancel_unpins_busy_peek() {
        let mut p = Presence::new();
        p.on(Event::Shortcut, at(0));
        p.on(Event::Busy(true), at(0));
        p.on(Event::FocusLost, at(0));
        assert_eq!(p.state(), State::Peek);
        p.on(Event::Cancelled, at(100));
        assert_eq!(p.state(), State::Sliver);
        assert_eq!(p.message(), None);
    }

    #[test]
    fn next_deadline_only_when_pending() {
        let mut p = Presence::new();
        assert_eq!(p.next_deadline(), None);
        p.on(Event::CursorEntered, at(0));
        assert_eq!(p.next_deadline(), Some(at(150)));
        p.on(Event::Tick, at(150));
        assert_eq!(p.next_deadline(), None);
    }
}
