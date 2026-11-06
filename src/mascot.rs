//! Canvas drawings: the notch silhouette and Bloop, the little mascot living in it.

use iced::{
    Color, Point, Rectangle, Renderer, Size, Theme, mouse,
    widget::canvas::{self, Action, Event, Frame, Geometry, LineCap, Path, Stroke},
};
use std::f32::consts::TAU;

/// What the mascot is feeling, derived from what the app is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mood {
    Idle,
    Typing,
    Thinking,
    Talking,
    Happy,
    Oops,
    Sleepy,
}

impl Mood {
    /// Moods that move on their own and need animation frames.
    pub fn animated(self) -> bool {
        matches!(
            self,
            Self::Thinking | Self::Talking | Self::Happy | Self::Oops
        )
    }
}

pub struct Mascot {
    pub mood: Mood,
    /// Seconds since start, drives motion.
    pub t: f32,
    /// Seconds since the current mood began, for one-shot motions.
    pub since: f32,
    pub blink: bool,
    pub body: Color,
    pub face: Color,
}

impl<Message> canvas::Program<Message> for Mascot {
    type State = ();

    fn update(
        &self,
        _state: &mut (),
        event: &Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        // Eyes follow the pointer, so repaint on movement (no timer needed).
        matches!(event, Event::Mouse(mouse::Event::CursorMoved { .. })).then(Action::request_redraw)
    }

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let side = bounds.width.min(bounds.height);
        // Design grid is 24×24 units, centered in the bounds.
        let u = side / 24.0;
        let ox = (bounds.width - side) / 2.0;
        let oy = (bounds.height - side) / 2.0;
        let t = self.t;

        let (mut dx, mut dy, mut stretch) = (0.0_f32, 0.0_f32, 0.0_f32);
        match self.mood {
            Mood::Thinking => {
                dy = (t * TAU * 0.8).sin() * 0.7;
                dx = (t * TAU * 0.4).sin() * 0.5;
            }
            Mood::Talking => {
                let beat = (t * TAU * 2.2).sin().abs();
                dy = -beat * 0.8;
                stretch = beat * 0.6;
            }
            Mood::Happy => {
                let hop = (self.since * TAU * 1.6).sin().abs() * (1.0 - self.since / 1.6).max(0.0);
                dy = -hop * 2.2;
                stretch = hop * 0.8;
            }
            Mood::Oops => {
                dx = (self.since * TAU * 7.0).sin() * 1.1 * (1.0 - self.since / 0.7).max(0.0);
            }
            Mood::Sleepy => dy = 0.6,
            Mood::Idle | Mood::Typing => {}
        }
        let p = |x: f32, y: f32| Point::new(ox + (x + dx) * u, oy + (y + dy) * u);

        // Antenna sprout.
        let stem = Stroke::default()
            .with_width(1.3 * u)
            .with_color(self.body)
            .with_line_cap(LineCap::Round);
        let tip = if self.mood == Mood::Sleepy {
            p(14.6, 2.6)
        } else {
            p(14.0, 1.6)
        };
        frame.stroke(&Path::line(p(12.0, 5.0 - stretch), tip), stem);
        frame.fill(&Path::circle(tip, 1.5 * u), self.body);

        // Body: a soft rounded square that stretches when excited.
        let top = 5.0 - stretch;
        frame.fill(
            &Path::rounded_rectangle(
                p(2.5, top),
                Size::new(19.0 * u, (21.5 - top) * u),
                (8.0 * u).into(),
            ),
            self.body,
        );

        // Where the eyes look.
        let mut look = match self.mood {
            Mood::Typing => (0.5, 1.0),
            Mood::Thinking => (0.9, -1.1),
            _ => (0.0, 0.0),
        };
        if matches!(self.mood, Mood::Idle | Mood::Typing)
            && let Some(pos) = cursor.position()
        {
            let center = Point::new(
                bounds.x + bounds.width / 2.0,
                bounds.y + bounds.height / 2.0,
            );
            let (vx, vy) = (pos.x - center.x, pos.y - center.y);
            let len = (vx * vx + vy * vy).sqrt().max(1.0);
            let reach = (len / (side * 2.0)).min(1.0) * 1.2;
            look = (vx / len * reach, vy / len * reach);
        }

        let eye_y = 12.5;
        let eyes = [8.5_f32, 15.5];
        let line = |w: f32| {
            Stroke::default()
                .with_width(w * u)
                .with_color(self.face)
                .with_line_cap(LineCap::Round)
        };
        match self.mood {
            Mood::Happy => {
                for x in eyes {
                    let arc = Path::new(|b| {
                        b.move_to(p(x - 1.8, eye_y + 0.8));
                        b.quadratic_curve_to(p(x, eye_y - 1.9), p(x + 1.8, eye_y + 0.8));
                    });
                    frame.stroke(&arc, line(1.4));
                }
            }
            Mood::Oops => {
                for (x, dir) in [(eyes[0], 1.0), (eyes[1], -1.0)] {
                    let chevron = Path::new(|b| {
                        b.move_to(p(x - 1.3 * dir, eye_y - 1.4));
                        b.line_to(p(x + 1.2 * dir, eye_y));
                        b.line_to(p(x - 1.3 * dir, eye_y + 1.4));
                    });
                    frame.stroke(&chevron, line(1.3));
                }
            }
            Mood::Sleepy => {
                for x in eyes {
                    let lid = Path::new(|b| {
                        b.move_to(p(x - 1.7, eye_y));
                        b.quadratic_curve_to(p(x, eye_y + 1.4), p(x + 1.7, eye_y));
                    });
                    frame.stroke(&lid, line(1.2));
                }
            }
            _ if self.blink => {
                for x in eyes {
                    frame.stroke(&Path::line(p(x - 1.6, eye_y), p(x + 1.6, eye_y)), line(1.2));
                }
            }
            _ => {
                for x in eyes {
                    frame.fill(
                        &Path::circle(p(x + look.0, eye_y + look.1), 1.9 * u),
                        self.face,
                    );
                    // Tiny highlight makes the eyes read as shiny.
                    frame.fill(
                        &Path::circle(p(x + look.0 + 0.6, eye_y + look.1 - 0.7), 0.55 * u),
                        self.body,
                    );
                }
            }
        }

        // Blush on happy faces.
        if self.mood == Mood::Happy {
            let blush = Color {
                a: 0.28,
                ..self.face
            };
            frame.fill(&Path::circle(p(5.6, 15.6), 1.3 * u), blush);
            frame.fill(&Path::circle(p(18.4, 15.6), 1.3 * u), blush);
        }

        // Mouth.
        let mouth_y = 16.6;
        match self.mood {
            Mood::Talking => {
                let open = 0.5 + (t * TAU * 2.2).sin().abs() * 1.8;
                frame.fill(
                    &Path::rounded_rectangle(
                        p(10.6, mouth_y - 0.6),
                        Size::new(2.8 * u, open * u),
                        (1.2 * u).into(),
                    ),
                    self.face,
                );
            }
            Mood::Happy => {
                let grin = Path::new(|b| {
                    b.move_to(p(9.8, mouth_y - 0.4));
                    b.quadratic_curve_to(p(12.0, mouth_y + 2.6), p(14.2, mouth_y - 0.4));
                    b.close();
                });
                frame.fill(&grin, self.face);
            }
            Mood::Oops => {
                let wobble = Path::new(|b| {
                    b.move_to(p(9.8, mouth_y + 0.6));
                    b.line_to(p(10.9, mouth_y - 0.2));
                    b.line_to(p(12.0, mouth_y + 0.6));
                    b.line_to(p(13.1, mouth_y - 0.2));
                    b.line_to(p(14.2, mouth_y + 0.6));
                });
                frame.stroke(&wobble, line(1.1));
            }
            Mood::Thinking => {
                frame.stroke(
                    &Path::line(p(11.2, mouth_y + 0.3), p(13.4, mouth_y)),
                    line(1.2),
                );
            }
            Mood::Sleepy => {
                frame.fill(&Path::circle(p(12.0, mouth_y + 0.4), 0.8 * u), self.face);
                // A little "z" drifting off the antenna.
                let z = Path::new(|b| {
                    b.move_to(p(18.2, 1.0));
                    b.line_to(p(20.6, 1.0));
                    b.line_to(p(18.2, 3.4));
                    b.line_to(p(20.6, 3.4));
                });
                frame.stroke(
                    &z,
                    Stroke::default()
                        .with_width(0.9 * u)
                        .with_color(self.body)
                        .with_line_cap(LineCap::Round),
                );
            }
            Mood::Idle | Mood::Typing => {
                let smile = Path::new(|b| {
                    b.move_to(p(10.4, mouth_y));
                    b.quadratic_curve_to(p(12.0, mouth_y + 1.5), p(13.6, mouth_y));
                });
                frame.stroke(&smile, line(1.2));
            }
        }

        vec![frame.into_geometry()]
    }
}

/// The notch silhouette as a polygon in logical units, used to clip the
/// native window so everything outside the shape is see-through.
pub fn notch_outline(size: Size, shoulder: f32, radius: f32) -> Vec<(f32, f32)> {
    let (w, h, s) = (size.width, size.height, shoulder);
    let r = radius.min((w - 2.0 * s) / 2.0).min(h - s).max(0.0);
    let mut points = Vec::with_capacity(48);
    let mut arc = |cx: f32, cy: f32, radius: f32, from: f32, to: f32| {
        const STEPS: usize = 10;
        for i in 0..=STEPS {
            let a = (from + (to - from) * i as f32 / STEPS as f32).to_radians();
            points.push((cx + radius * a.cos(), cy + radius * a.sin()));
        }
    };
    // Screen y grows downward, so 90° points down.
    arc(0.0, s, s, -90.0, 0.0); // left shoulder (concave)
    arc(s + r, h - r, r, 180.0, 90.0); // bottom-left corner
    arc(w - s - r, h - r, r, 90.0, 0.0); // bottom-right corner
    arc(w, s, s, 180.0, 270.0); // right shoulder (concave)
    points
}

/// The notch silhouette: flush with the screen's top edge, with concave
/// "shoulders" that melt into the bezel and a rounded bottom.
pub struct Notch {
    pub fill: Color,
    pub outline: Option<Color>,
    pub shoulder: f32,
    pub radius: f32,
}

impl Notch {
    fn path(&self, size: Size, closed: bool) -> Path {
        let (w, h) = (size.width, size.height);
        let s = self.shoulder;
        let r = self.radius.min((w - 2.0 * s) / 2.0).min(h - s).max(0.0);
        // Cubic Bézier constant approximating a quarter circle.
        let k = 0.552_284_8;
        Path::new(|b| {
            b.move_to(Point::new(0.0, 0.0));
            b.bezier_curve_to(
                Point::new(s * k, 0.0),
                Point::new(s, s * (1.0 - k)),
                Point::new(s, s),
            );
            b.line_to(Point::new(s, h - r));
            b.bezier_curve_to(
                Point::new(s, h - r + r * k),
                Point::new(s + r - r * k, h),
                Point::new(s + r, h),
            );
            b.line_to(Point::new(w - s - r, h));
            b.bezier_curve_to(
                Point::new(w - s - r + r * k, h),
                Point::new(w - s, h - r + r * k),
                Point::new(w - s, h - r),
            );
            b.line_to(Point::new(w - s, s));
            b.bezier_curve_to(
                Point::new(w - s, s * (1.0 - k)),
                Point::new(w - s * k, 0.0),
                Point::new(w, 0.0),
            );
            if closed {
                b.close();
            }
        })
    }
}

impl<Message> canvas::Program<Message> for Notch {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        frame.fill(&self.path(bounds.size(), true), self.fill);
        if let Some(color) = self.outline {
            // Inset half a pixel so the hairline stays inside the window.
            let inner = Size::new(bounds.width, bounds.height - 0.5);
            frame.stroke(
                &self.path(inner, false),
                Stroke::default().with_width(1.0).with_color(color),
            );
        }
        vec![frame.into_geometry()]
    }
}
