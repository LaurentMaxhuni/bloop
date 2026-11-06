//! shadcn/ui "neutral" design tokens and widget styles for the notch.

use iced::{
    Background, Border, Color, Font, Shadow, Theme, Vector,
    font::{Family, Weight},
    widget::{button, container, overlay::menu, pick_list, scrollable, text_input},
};

pub const REGULAR: Font = Font {
    family: Family::Name("Segoe UI"),
    ..Font::DEFAULT
};
pub const MEDIUM: Font = Font {
    family: Family::Name("Segoe UI"),
    weight: Weight::Semibold,
    ..Font::DEFAULT
};

/// Corner radius used by shadcn `rounded-md` controls.
pub const RADIUS: f32 = 8.0;

const fn hex(rgb: u32) -> Color {
    Color::from_rgb(
        ((rgb >> 16) & 0xff) as f32 / 255.0,
        ((rgb >> 8) & 0xff) as f32 / 255.0,
        (rgb & 0xff) as f32 / 255.0,
    )
}

#[derive(Debug, Clone, Copy)]
pub struct Tokens {
    pub background: Color,
    pub foreground: Color,
    pub muted_foreground: Color,
    pub border: Color,
    pub ring: Color,
    pub primary: Color,
    pub primary_foreground: Color,
    pub secondary: Color,
    pub accent: Color,
    pub destructive: Color,
}

impl Tokens {
    pub const fn new(light: bool) -> Self {
        if light {
            Self {
                background: hex(0xffffff),
                foreground: hex(0x0a0a0a),
                muted_foreground: hex(0x737373),
                border: hex(0xe5e5e5),
                ring: hex(0xa3a3a3),
                primary: hex(0x171717),
                primary_foreground: hex(0xfafafa),
                secondary: hex(0xf5f5f5),
                accent: hex(0xf5f5f5),
                destructive: hex(0xdc2626),
            }
        } else {
            Self {
                background: hex(0x0a0a0a),
                foreground: hex(0xfafafa),
                muted_foreground: hex(0xa3a3a3),
                border: hex(0x262626),
                ring: hex(0x737373),
                primary: hex(0xfafafa),
                primary_foreground: hex(0x171717),
                secondary: hex(0x262626),
                accent: hex(0x262626),
                destructive: hex(0xf87171),
            }
        }
    }
}

fn mix(a: Color, b: Color, t: f32) -> Color {
    Color::from_rgba(
        a.r + (b.r - a.r) * t,
        a.g + (b.g - a.g) * t,
        a.b + (b.b - a.b) * t,
        a.a + (b.a - a.a) * t,
    )
}

fn fade(color: Color, alpha: f32) -> Color {
    Color {
        a: color.a * alpha,
        ..color
    }
}

fn border(color: Color, width: f32) -> Border {
    Border {
        color,
        width,
        radius: RADIUS.into(),
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Variant {
    Primary,
    Secondary,
    Outline,
    Ghost,
    Destructive,
    /// Invisible chrome used for the clickable compact notch.
    Bare,
}

pub fn btn(t: Tokens, variant: Variant) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let (background, text_color, edge) = match variant {
            Variant::Primary => (
                Some(if hovered {
                    mix(t.primary, t.background, 0.12)
                } else {
                    t.primary
                }),
                t.primary_foreground,
                Color::TRANSPARENT,
            ),
            Variant::Secondary => (
                Some(if hovered {
                    mix(t.secondary, t.foreground, 0.06)
                } else {
                    t.secondary
                }),
                t.foreground,
                Color::TRANSPARENT,
            ),
            Variant::Outline => (
                Some(if hovered { t.accent } else { t.background }),
                t.foreground,
                t.border,
            ),
            Variant::Ghost => (
                hovered.then_some(t.accent),
                if hovered {
                    t.foreground
                } else {
                    t.muted_foreground
                },
                Color::TRANSPARENT,
            ),
            Variant::Destructive => (
                Some(if hovered {
                    mix(t.destructive, t.background, 0.15)
                } else {
                    t.destructive
                }),
                hex(0xffffff),
                Color::TRANSPARENT,
            ),
            Variant::Bare => (None, t.foreground, Color::TRANSPARENT),
        };
        let alpha = if matches!(status, button::Status::Disabled) {
            0.5
        } else {
            1.0
        };
        button::Style {
            background: background.map(|c| Background::Color(fade(c, alpha))),
            text_color: fade(text_color, alpha),
            border: border(fade(edge, alpha), 1.0),
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

pub fn input(t: Tokens) -> impl Fn(&Theme, text_input::Status) -> text_input::Style {
    move |_, status| text_input::Style {
        background: Background::Color(Color::TRANSPARENT),
        border: border(
            match status {
                text_input::Status::Focused { .. } => t.ring,
                _ => t.border,
            },
            1.0,
        ),
        icon: t.muted_foreground,
        placeholder: t.muted_foreground,
        value: t.foreground,
        selection: fade(t.ring, 0.45),
    }
}

/// A text input without chrome, used inside the composer card.
pub fn bare_input(t: Tokens) -> impl Fn(&Theme, text_input::Status) -> text_input::Style {
    move |theme, status| text_input::Style {
        border: Border::default(),
        ..input(t)(theme, status)
    }
}

pub fn select(t: Tokens) -> impl Fn(&Theme, pick_list::Status) -> pick_list::Style {
    move |_, status| pick_list::Style {
        text_color: t.foreground,
        placeholder_color: t.muted_foreground,
        handle_color: t.muted_foreground,
        background: Background::Color(if matches!(status, pick_list::Status::Hovered) {
            fade(t.accent, 0.6)
        } else {
            Color::TRANSPARENT
        }),
        border: border(
            if matches!(status, pick_list::Status::Opened { .. }) {
                t.ring
            } else {
                t.border
            },
            1.0,
        ),
    }
}

pub fn menu(t: Tokens) -> impl Fn(&Theme) -> menu::Style {
    move |_| menu::Style {
        background: Background::Color(t.background),
        border: border(t.border, 1.0),
        text_color: t.foreground,
        selected_text_color: t.foreground,
        selected_background: Background::Color(t.accent),
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.18),
            offset: Vector::new(0.0, 4.0),
            blur_radius: 12.0,
        },
    }
}

pub fn scroll(t: Tokens) -> impl Fn(&Theme, scrollable::Status) -> scrollable::Style {
    move |theme, status| {
        let mut style = scrollable::default(theme, status);
        let rail = scrollable::Rail {
            background: None,
            border: Border::default(),
            scroller: scrollable::Scroller {
                background: Background::Color(t.border),
                border: Border {
                    radius: 999.0.into(),
                    ..Border::default()
                },
            },
        };
        style.container = container::Style::default();
        style.vertical_rail = rail;
        style.horizontal_rail = rail;
        style
    }
}

/// A shadcn card / alert surface.
pub fn card(t: Tokens, fill: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(fill.into()),
        border: Border {
            color: t.border,
            width: 1.0,
            radius: 12.0.into(),
        },
        ..Default::default()
    }
}

/// Rounded, outlined pill (shadcn `Badge variant="outline"`).
pub fn badge(t: Tokens) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        text_color: Some(t.muted_foreground),
        border: Border {
            color: t.border,
            width: 1.0,
            radius: 999.0.into(),
        },
        ..Default::default()
    }
}

/// Filled chat bubble for the user's own messages.
pub fn bubble(t: Tokens) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(t.secondary.into()),
        text_color: Some(t.foreground),
        border: Border {
            radius: 14.0.into(),
            ..Border::default()
        },
        ..Default::default()
    }
}

pub fn separator(t: Tokens) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(t.border.into()),
        ..Default::default()
    }
}
