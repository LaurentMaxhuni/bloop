//! Canvas drawings: the notch silhouette and Bloop, the little mascot living in it.

use bloop::config::{Coat, Eyewear, Hat, Look, Outfit};
use iced::{
    Color, Point, Rectangle, Renderer, Size, Theme, mouse,
    widget::canvas::{self, Action, Event, Frame, Geometry, LineCap, Path, Stroke},
};
use std::f32::consts::TAU;

/// Body and face colours: a tinted coat keeps a dark face in either theme.
fn colors(look: Look, body: Color, face: Color) -> (Color, Color) {
    let ink = rgb(0.10, 0.10, 0.13);
    match look.coat {
        Coat::Classic => (body, face),
        Coat::Mint => (rgb(0.55, 0.86, 0.72), ink),
        Coat::Peach => (rgb(0.98, 0.70, 0.58), ink),
        Coat::Lilac => (rgb(0.76, 0.66, 0.96), ink),
        Coat::Sky => (rgb(0.55, 0.78, 0.98), ink),
        Coat::Lemon => (rgb(0.98, 0.89, 0.50), ink),
    }
}

/// Grid units added above Bloop so the hat (and a happy hop) stays inside the canvas.
fn headroom(hat: Hat) -> f32 {
    match hat {
        Hat::None => 0.0,
        Hat::Cap | Hat::Beanie | Hat::Crown => 4.0,
        Hat::TopHat | Hat::Party => 6.5,
        Hat::Wizard => 8.0,
    }
}

/// Fixed accessory colours, chosen to read on light and dark bodies alike.
const GOLD: Color = Color::from_rgb(0.95, 0.74, 0.25);
const RED: Color = Color::from_rgb(0.88, 0.27, 0.33);
const PURPLE: Color = Color::from_rgb(0.38, 0.28, 0.74);
const SHADE: Color = Color::from_rgb(0.07, 0.07, 0.09);

fn rgb(r: f32, g: f32, b: f32) -> Color {
    Color::from_rgb(r, g, b)
}

/// The left edge of Bloop's body at height `y`: the bottom corners are quarter circles of
/// radius 8 centred at (10.5, 13.5) and (13.5, 13.5).
fn body_left(y: f32) -> f32 {
    let dy = (y - 13.5).max(0.0);
    10.5 - (64.0 - dy * dy).max(0.0).sqrt()
}

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
    /// Wide-eyed little hop when the cursor arrives.
    Perk,
    /// Squeezed-shut eyes and a big open mouth: the reply was funny.
    Laughing,
    /// Frowning brows and a little anger mark: the reply failed or was stopped.
    Mad,
}

impl Mood {
    /// Moods that move on their own and need animation frames.
    pub fn animated(self) -> bool {
        matches!(
            self,
            Self::Thinking
                | Self::Talking
                | Self::Happy
                | Self::Oops
                | Self::Perk
                | Self::Laughing
                | Self::Mad
        )
    }

    /// How Bloop feels about a finished reply: mad when it broke off, laughing when it
    /// was meant to be funny, happy otherwise.
    pub fn for_reply(prompt: &str, reply: &str, complete: bool) -> Self {
        if !complete {
            return Self::Mad;
        }
        let words = |text: &str| {
            text.to_lowercase()
                .split(|c: char| !c.is_alphanumeric())
                .filter(|w| !w.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>()
        };
        let laughing = words(reply).iter().any(|w| {
            w.starts_with("haha") || w.starts_with("hehe") || matches!(w.as_str(), "lol" | "lmao")
        }) || ['😂', '🤣', '😆', '😄', '😹']
            .iter()
            .any(|e| reply.contains(*e));
        let asked = words(prompt).iter().any(|w| {
            matches!(
                w.as_str(),
                "joke" | "jokes" | "pun" | "puns" | "funny" | "laugh" | "meme" | "roast"
            )
        });
        if laughing || asked {
            Self::Laughing
        } else {
            Self::Happy
        }
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
    /// Cheek colour on happy faces; `None` keeps a soft tint of the face colour.
    pub blush: Option<Color>,
    pub look: Look,
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
        // Design grid is 24×24 units, centered in the bounds. A hat needs headroom, so the
        // grid grows to 24 + HEADROOM units tall and Bloop shrinks a little to make room.
        let room = headroom(self.look.hat);
        let u = side / (24.0 + room);
        let ox = (bounds.width - 24.0 * u) / 2.0;
        let oy = (bounds.height - side) / 2.0 + room * u;
        let t = self.t;
        let (body, face) = colors(self.look, self.body, self.face);

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
            Mood::Laughing => {
                // Fast giggly bounce that settles after a couple of seconds.
                let fade = (1.0 - self.since / 2.4).max(0.0);
                let shake = (self.since * TAU * 3.2).sin().abs() * fade;
                dy = -shake * 1.4;
                stretch = shake * 0.7;
            }
            Mood::Mad => {
                // A huffy tremble, then it just glares.
                dx = (self.since * TAU * 9.0).sin() * 0.6 * (1.0 - self.since / 0.9).max(0.0);
                dy = 0.3;
            }
            Mood::Sleepy => dy = 0.6,
            Mood::Perk => {
                let hop = (self.since * TAU * 0.85).sin().abs() * (1.0 - self.since / 0.6).max(0.0);
                dy = -hop * 1.6;
                stretch = hop * 0.5;
            }
            Mood::Idle | Mood::Typing => {}
        }
        let p = |x: f32, y: f32| Point::new(ox + (x + dx) * u, oy + (y + dy) * u);

        // Antenna sprout; a hat covers it.
        let has_hat = self.look.hat != Hat::None;
        let stem = Stroke::default()
            .with_width(1.3 * u)
            .with_color(body)
            .with_line_cap(LineCap::Round);
        let tip = if self.mood == Mood::Sleepy {
            p(14.6, 2.6)
        } else {
            p(14.0, 1.6)
        };
        if !has_hat {
            frame.stroke(&Path::line(p(12.0, 5.0 - stretch), tip), stem);
            frame.fill(&Path::circle(tip, 1.5 * u), body);
        }

        // A cape hangs behind the body, so it goes first.
        if self.look.outfit == Outfit::Cape {
            let cape = Path::new(|b| {
                b.move_to(p(4.0, 12.0));
                b.line_to(p(20.0, 12.0));
                b.quadratic_curve_to(p(22.6, 18.0), p(23.4, 23.4));
                b.line_to(p(0.6, 23.4));
                b.quadratic_curve_to(p(1.4, 18.0), p(4.0, 12.0));
                b.close();
            });
            frame.fill(&cape, RED);
        }

        // Body: a soft rounded square that stretches when excited.
        let top = 5.0 - stretch;
        frame.fill(
            &Path::rounded_rectangle(
                p(2.5, top),
                Size::new(19.0 * u, (21.5 - top) * u),
                (8.0 * u).into(),
            ),
            body,
        );
        draw_outfit(&mut frame, &p, u, self.look.outfit, body);

        // Where the eyes look.
        let mut look = match self.mood {
            Mood::Typing => (0.5, 1.0),
            Mood::Thinking => (0.9, -1.1),
            _ => (0.0, 0.0),
        };
        if matches!(self.mood, Mood::Idle | Mood::Typing | Mood::Perk)
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
                .with_color(face)
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
            Mood::Laughing => {
                // Squeezed shut: tight upside-down "v" shapes.
                for x in eyes {
                    let squint = Path::new(|b| {
                        b.move_to(p(x - 1.7, eye_y + 0.9));
                        b.line_to(p(x, eye_y - 0.9));
                        b.line_to(p(x + 1.7, eye_y + 0.9));
                    });
                    frame.stroke(&squint, line(1.3));
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
                let eye_r = match self.mood {
                    Mood::Perk => 2.3,
                    Mood::Mad => 1.6,
                    _ => 1.9,
                };
                for x in eyes {
                    frame.fill(
                        &Path::circle(p(x + look.0, eye_y + look.1), eye_r * u),
                        face,
                    );
                    // Tiny highlight makes the eyes read as shiny.
                    frame.fill(
                        &Path::circle(p(x + look.0 + 0.6, eye_y + look.1 - 0.7), 0.55 * u),
                        body,
                    );
                }
            }
        }

        // Mad: brows slanting down toward the middle, and an anger mark by the antenna.
        if self.mood == Mood::Mad {
            frame.stroke(&Path::line(p(6.3, 8.9), p(10.5, 10.4)), line(1.3));
            frame.stroke(&Path::line(p(13.5, 10.4), p(17.7, 8.9)), line(1.3));
            let mark = Stroke::default()
                .with_width(0.8 * u)
                .with_color(body)
                .with_line_cap(LineCap::Round);
            let (cx, cy) = (19.6, 2.6);
            for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                let tick = Path::new(|b| {
                    b.move_to(p(cx + sx * 0.5, cy + sy * 1.4));
                    b.quadratic_curve_to(
                        p(cx + sx * 0.5, cy + sy * 0.5),
                        p(cx + sx * 1.4, cy + sy * 0.5),
                    );
                });
                frame.stroke(&tick, mark);
            }
        }

        // Blush on happy faces.
        if matches!(self.mood, Mood::Happy | Mood::Laughing) {
            let blush = Color {
                a: 0.28,
                ..self.blush.unwrap_or(face)
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
                    face,
                );
            }
            Mood::Happy => {
                let grin = Path::new(|b| {
                    b.move_to(p(9.8, mouth_y - 0.4));
                    b.quadratic_curve_to(p(12.0, mouth_y + 2.6), p(14.2, mouth_y - 0.4));
                    b.close();
                });
                frame.fill(&grin, face);
            }
            Mood::Laughing => {
                // Wide open "D" mouth that flaps with the giggle.
                let fade = (1.0 - self.since / 2.4).max(0.0);
                let open = 2.2 + (self.since * TAU * 3.2).sin().abs() * 1.2 * fade;
                let laugh = Path::new(|b| {
                    b.move_to(p(9.4, mouth_y - 0.8));
                    b.line_to(p(14.6, mouth_y - 0.8));
                    b.quadratic_curve_to(
                        p(12.0, mouth_y - 0.8 + open * 1.6),
                        p(9.4, mouth_y - 0.8),
                    );
                    b.close();
                });
                frame.fill(&laugh, face);
            }
            Mood::Mad => {
                let frown = Path::new(|b| {
                    b.move_to(p(10.0, mouth_y + 0.9));
                    b.quadratic_curve_to(p(12.0, mouth_y - 0.7), p(14.0, mouth_y + 0.9));
                });
                frame.stroke(&frown, line(1.2));
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
                frame.fill(&Path::circle(p(12.0, mouth_y + 0.4), 0.8 * u), face);
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
                        .with_color(body)
                        .with_line_cap(LineCap::Round),
                );
            }
            Mood::Perk => {
                frame.fill(&Path::circle(p(12.0, mouth_y + 0.3), 1.0 * u), face);
            }
            Mood::Idle | Mood::Typing => {
                let smile = Path::new(|b| {
                    b.move_to(p(10.4, mouth_y));
                    b.quadratic_curve_to(p(12.0, mouth_y + 1.5), p(13.6, mouth_y));
                });
                frame.stroke(&smile, line(1.2));
            }
        }

        let top = 5.0 - stretch;
        draw_eyewear(&mut frame, &p, u, self.look.eyewear);
        draw_hat(&mut frame, &p, u, self.look.hat, top);

        vec![frame.into_geometry()]
    }
}

fn stroke(color: Color, width: f32) -> Stroke<'static> {
    Stroke::default()
        .with_width(width)
        .with_color(color)
        .with_line_cap(LineCap::Round)
}

fn rounded(p: &impl Fn(f32, f32) -> Point, x: f32, y: f32, w: f32, h: f32, r: f32, u: f32) -> Path {
    Path::rounded_rectangle(p(x, y), Size::new(w * u, h * u), (r * u).into())
}

fn draw_eyewear(frame: &mut Frame, p: &impl Fn(f32, f32) -> Point, u: f32, eyewear: Eyewear) {
    let eye_y = 12.5;
    match eyewear {
        Eyewear::None => {}
        Eyewear::Sunglasses => {
            for x in [5.4_f32, 12.6] {
                frame.fill(&rounded(p, x, 10.5, 6.0, 4.0, 1.7, u), SHADE);
                // A glint so the lenses read as glass.
                frame.stroke(
                    &Path::line(p(x + 1.2, 11.9), p(x + 2.2, 11.1)),
                    stroke(Color::from_rgba(1.0, 1.0, 1.0, 0.7), 0.6 * u),
                );
            }
            frame.stroke(
                &Path::line(p(11.2, 11.4), p(12.8, 11.4)),
                stroke(SHADE, 0.8 * u),
            );
            frame.stroke(
                &Path::line(p(5.6, 11.4), p(2.8, 10.9)),
                stroke(SHADE, 0.7 * u),
            );
            frame.stroke(
                &Path::line(p(18.4, 11.4), p(21.2, 10.9)),
                stroke(SHADE, 0.7 * u),
            );
        }
        Eyewear::Round => {
            for x in [8.5_f32, 15.5] {
                frame.stroke(&Path::circle(p(x, eye_y), 2.9 * u), stroke(GOLD, 0.7 * u));
            }
            let bridge = Path::new(|b| {
                b.move_to(p(11.4, eye_y - 0.3));
                b.quadratic_curve_to(p(12.0, eye_y - 1.2), p(12.6, eye_y - 0.3));
            });
            frame.stroke(&bridge, stroke(GOLD, 0.6 * u));
        }
        Eyewear::Monocle => {
            frame.stroke(
                &Path::circle(p(15.5, eye_y), 3.0 * u),
                stroke(GOLD, 0.8 * u),
            );
            let chain = Path::new(|b| {
                b.move_to(p(17.6, eye_y + 2.2));
                b.quadratic_curve_to(p(19.6, eye_y + 5.5), p(18.8, eye_y + 8.0));
            });
            frame.stroke(&chain, stroke(GOLD, 0.4 * u));
        }
    }
}

/// A strip of the body between heights `y0` and `y1`, following its rounded bottom.
fn body_band(p: &impl Fn(f32, f32) -> Point, y0: f32, y1: f32) -> Path {
    const STEPS: usize = 8;
    let at = |i: usize| y0 + (y1 - y0) * i as f32 / STEPS as f32;
    Path::new(|b| {
        b.move_to(p(body_left(y0), y0));
        for i in 1..=STEPS {
            b.line_to(p(body_left(at(i)), at(i)));
        }
        for i in (0..=STEPS).rev() {
            b.line_to(p(24.0 - body_left(at(i)), at(i)));
        }
        b.close();
    })
}

fn draw_outfit(
    frame: &mut Frame,
    p: &impl Fn(f32, f32) -> Point,
    u: f32,
    outfit: Outfit,
    body: Color,
) {
    match outfit {
        Outfit::None => {}
        Outfit::BowTie => {
            for dir in [-1.0_f32, 1.0] {
                let wing = Path::new(|b| {
                    b.move_to(p(12.0, 20.0));
                    b.line_to(p(12.0 + dir * 3.0, 18.6));
                    b.line_to(p(12.0 + dir * 3.0, 21.4));
                    b.close();
                });
                frame.fill(&wing, RED);
            }
            frame.fill(&Path::circle(p(12.0, 20.0), 0.9 * u), RED);
        }
        Outfit::Scarf => {
            frame.fill(&body_band(p, 18.4, 20.4), RED);
            for x in [7.0_f32, 11.0, 15.0] {
                frame.stroke(
                    &Path::line(p(x, 18.6), p(x + 1.0, 20.2)),
                    stroke(Color::from_rgba(1.0, 1.0, 1.0, 0.55), 0.5 * u),
                );
            }
            frame.fill(&rounded(p, 15.6, 19.6, 2.4, 4.2, 0.8, u), RED);
        }
        Outfit::Robe => {
            frame.fill(&body_band(p, 18.0, 21.5), PURPLE);
            let neck = Path::new(|b| {
                b.move_to(p(10.4, 18.0));
                b.line_to(p(13.6, 18.0));
                b.line_to(p(12.0, 20.0));
                b.close();
            });
            frame.fill(&neck, body);
            frame.stroke(
                &Path::line(p(10.4, 18.0), p(12.0, 20.0)),
                stroke(GOLD, 0.5 * u),
            );
            frame.stroke(
                &Path::line(p(13.6, 18.0), p(12.0, 20.0)),
                stroke(GOLD, 0.5 * u),
            );
            frame.fill(&Path::circle(p(17.0, 19.8), 0.6 * u), GOLD);
            frame.fill(&Path::circle(p(7.0, 20.2), 0.45 * u), GOLD);
        }
        Outfit::Cape => {
            // The cape itself is behind the body; here only its clasp shows.
            frame.stroke(
                &Path::line(p(5.2, 18.6), p(18.8, 18.6)),
                stroke(GOLD, 0.5 * u),
            );
            frame.fill(&Path::circle(p(12.0, 18.8), 0.9 * u), GOLD);
        }
    }
}

fn draw_hat(frame: &mut Frame, p: &impl Fn(f32, f32) -> Point, u: f32, hat: Hat, top: f32) {
    match hat {
        Hat::None => {}
        Hat::TopHat => {
            let dark = rgb(0.13, 0.13, 0.16);
            frame.fill(&rounded(p, 7.6, top - 7.6, 8.8, 8.0, 0.8, u), dark);
            frame.fill(
                &Path::rectangle(p(7.6, top - 2.0), Size::new(8.8 * u, 1.5 * u)),
                RED,
            );
            frame.fill(&rounded(p, 4.6, top - 0.2, 14.8, 1.8, 0.9, u), dark);
        }
        Hat::Beanie => {
            frame.fill(
                &rounded(p, 4.4, top - 3.6, 15.2, 6.0, 5.5, u),
                rgb(0.22, 0.62, 0.62),
            );
            frame.fill(
                &rounded(p, 3.6, top + 1.0, 16.8, 2.4, 1.2, u),
                rgb(0.30, 0.74, 0.72),
            );
            frame.fill(
                &Path::circle(p(12.0, top - 4.2), 1.7 * u),
                rgb(0.95, 0.95, 0.93),
            );
        }
        Hat::Cap => {
            let blue = rgb(0.24, 0.45, 0.92);
            let dome = Path::new(|b| {
                b.move_to(p(4.0, top + 2.6));
                b.quadratic_curve_to(p(4.6, top - 3.6), p(12.0, top - 3.4));
                b.quadratic_curve_to(p(19.4, top - 3.6), p(20.0, top + 2.6));
                b.close();
            });
            frame.fill(&dome, blue);
            let visor = Path::new(|b| {
                b.move_to(p(12.0, top + 1.6));
                b.line_to(p(20.0, top + 1.6));
                b.quadratic_curve_to(p(24.0, top + 2.0), p(23.6, top + 3.2));
                b.line_to(p(12.0, top + 3.0));
                b.close();
            });
            frame.fill(&visor, rgb(0.17, 0.33, 0.72));
            frame.fill(&Path::circle(p(12.0, top - 3.3), 0.8 * u), blue);
        }
        Hat::Crown => {
            let crown = Path::new(|b| {
                b.move_to(p(6.0, top + 1.6));
                b.line_to(p(5.4, top - 3.6));
                b.line_to(p(8.7, top - 1.0));
                b.line_to(p(12.0, top - 4.4));
                b.line_to(p(15.3, top - 1.0));
                b.line_to(p(18.6, top - 3.6));
                b.line_to(p(18.0, top + 1.6));
                b.close();
            });
            frame.fill(&crown, GOLD);
            for (x, color) in [(8.6, RED), (12.0, rgb(0.25, 0.55, 0.95)), (15.4, RED)] {
                frame.fill(&Path::circle(p(x, top + 0.2), 0.7 * u), color);
            }
        }
        Hat::Party => {
            let yellow = rgb(1.0, 0.86, 0.35);
            let cone = Path::new(|b| {
                b.move_to(p(7.6, top + 1.4));
                b.line_to(p(12.6, top - 7.4));
                b.line_to(p(16.6, top + 1.4));
                b.close();
            });
            frame.fill(&cone, rgb(0.96, 0.45, 0.70));
            for (y, half) in [(top - 1.6, 3.6_f32), (top - 4.6, 2.0)] {
                frame.stroke(
                    &Path::line(p(12.2 - half, y), p(12.4 + half, y)),
                    stroke(yellow, 0.7 * u),
                );
            }
            frame.fill(&Path::circle(p(12.6, top - 7.6), 1.2 * u), yellow);
        }
        Hat::Wizard => {
            let cone = Path::new(|b| {
                b.move_to(p(6.0, top + 1.4));
                b.quadratic_curve_to(p(10.0, top - 5.0), p(14.6, top - 9.4));
                b.quadratic_curve_to(p(14.2, top - 4.0), p(18.0, top + 1.4));
                b.close();
            });
            frame.fill(&cone, PURPLE);
            frame.fill(&rounded(p, 3.2, top + 0.4, 17.6, 2.0, 1.0, u), PURPLE);
            for (x, y, r) in [
                (11.6, top - 2.6, 0.7),
                (14.4, top - 0.8, 0.5),
                (13.4, top - 5.4, 0.45),
            ] {
                frame.fill(&Path::circle(p(x, y), r * u), GOLD);
            }
        }
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
