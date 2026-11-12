//! Assistant replies as rich content: Markdown with syntax-highlighted code blocks and
//! laid-out math. A [`Reply`] keeps its parsed Markdown and extends it as text streams in.

use super::Message;
use crate::icons::{self, Icon};
use crate::style::{RADIUS, Tokens};
use bloop::math::{self, Limits, Node, Spacing};
use iced::{
    Alignment, Background, Border, Color, Element, Font, Length, Padding,
    font::{Family, Weight},
    widget::{
        Space, button, column, container, markdown, rich_text, row, scrollable, text, tooltip,
    },
};

const MATH: Font = Font {
    family: Family::Name("Cambria Math"),
    ..Font::DEFAULT
};
const MATH_BOLD: Font = Font {
    family: Family::Name("Cambria Math"),
    weight: Weight::Bold,
    ..Font::DEFAULT
};
const MONO: Font = Font {
    family: Family::Name("Consolas"),
    ..Font::DEFAULT
};

/// One assistant reply, parsed. Updating with text that extends the previous text only
/// parses what was added, so streaming stays cheap.
#[derive(Default)]
pub struct Reply {
    source: String,
    prepared: String,
    content: markdown::Content,
}

impl Reply {
    pub fn new(text: &str) -> Self {
        let mut reply = Self::default();
        reply.update(text);
        reply
    }

    pub fn update(&mut self, text: &str) {
        if text == self.source {
            return;
        }
        let prepared = math::prepare(text);
        match prepared.strip_prefix(self.prepared.as_str()) {
            Some(added) if text.starts_with(self.source.as_str()) => self.content.push_str(added),
            // A formula closed or a fence changed what came before: parse it all again.
            _ => self.content = markdown::Content::parse(&prepared),
        }
        self.source = text.to_owned();
        self.prepared = prepared;
    }

    pub fn view<'a>(
        &'a self,
        t: Tokens,
        copied: Option<&'a str>,
        icons_ready: bool,
    ) -> Element<'a, Message> {
        markdown::view_with(
            self.content.items(),
            settings(t),
            &Viewer {
                t,
                copied,
                icons_ready,
            },
        )
    }
}

fn settings(t: Tokens) -> markdown::Settings {
    let style = markdown::Style {
        font: Font::DEFAULT,
        inline_code_highlight: markdown::Highlight {
            background: Background::Color(t.secondary),
            border: Border {
                radius: 4.0.into(),
                ..Border::default()
            },
        },
        inline_code_padding: Padding::from([0, 3]),
        inline_code_color: t.foreground,
        inline_code_font: MONO,
        code_block_font: MONO,
        link_color: link(t),
    };
    let mut settings = markdown::Settings::with_text_size(14, style);
    settings.h1_size = 20.into();
    settings.h2_size = 18.into();
    settings.h3_size = 16.into();
    settings.h4_size = 14.into();
    settings.code_size = 12.5.into();
    settings.spacing = 10.into();
    settings
}

/// Links in the accent colour; a soft blue when the accent is the neutral black/white.
fn link(t: Tokens) -> Color {
    if t.primary == t.foreground || t.primary == Color::BLACK || t.primary == Color::WHITE {
        Color::from_rgb8(0x60, 0xa5, 0xfa)
    } else {
        t.primary
    }
}

/// Code blocks sit on a dark panel in both themes: the highlighter's palette is a dark one.
const CODE_BG: Color = Color::from_rgb(0.09, 0.09, 0.11);
const CODE_FG: Color = Color::from_rgb(0.87, 0.87, 0.89);
const CODE_MUTED: Color = Color::from_rgb(0.55, 0.55, 0.6);

struct Viewer<'a> {
    t: Tokens,
    /// The code block most recently copied, so its button can say so.
    copied: Option<&'a str>,
    icons_ready: bool,
}

impl<'a> markdown::Viewer<'a, Message> for Viewer<'a> {
    fn on_link_click(url: markdown::Uri) -> Message {
        Message::OpenLink(url)
    }

    fn code_block(
        &self,
        settings: markdown::Settings,
        language: Option<&'a str>,
        code: &'a str,
        lines: &'a [markdown::Text],
    ) -> Element<'a, Message> {
        if language == Some("math") {
            return display_math(code, self.t);
        }
        let copied = self.copied == Some(code.trim_end());
        let word = if copied { "Copied" } else { "Copy" };
        let label = if self.icons_ready {
            text(
                if copied { Icon::Check } else { Icon::Copy }
                    .codepoint()
                    .to_string(),
            )
            .font(icons::FONT)
            .size(13)
        } else {
            text(word).size(12)
        };
        let copy = button(label)
            .padding(if self.icons_ready { [3, 6] } else { [2, 8] })
            .on_press(Message::CopyCode(code.trim_end().to_owned()))
            .style(move |_, status| button::Style {
                background: matches!(status, button::Status::Hovered | button::Status::Pressed)
                    .then_some(Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.08))),
                text_color: if copied { CODE_FG } else { CODE_MUTED },
                border: Border {
                    radius: 6.0.into(),
                    ..Border::default()
                },
                ..button::Style::default()
            });
        let header = row![
            text(language.filter(|l| !l.is_empty()).unwrap_or("code"))
                .size(11)
                .font(MONO)
                .color(CODE_MUTED),
            Space::new().width(Length::Fill),
            tooltip(
                copy,
                container(text(word).size(12).color(CODE_FG))
                    .padding([4, 8])
                    .style(|_| container::Style {
                        background: Some(CODE_BG.into()),
                        ..Default::default()
                    }),
                tooltip::Position::Bottom,
            ),
        ]
        .align_y(Alignment::Center);
        let body = column(lines.iter().map(|line| {
            rich_text(line.spans(settings.style))
                .on_link_click(Message::OpenLink)
                .font(MONO)
                .size(settings.code_size)
                .color(CODE_FG)
                .into()
        }));
        container(
            column![
                container(header).padding(Padding {
                    top: 4.0,
                    bottom: 0.0,
                    left: 12.0,
                    right: 6.0,
                }),
                scrollable(container(body).padding(Padding {
                    top: 4.0,
                    bottom: 12.0,
                    left: 12.0,
                    right: 12.0,
                }))
                .direction(scrollable::Direction::Horizontal(
                    scrollable::Scrollbar::new().width(4).scroller_width(4),
                )),
            ]
            .spacing(2),
        )
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(CODE_BG)),
            border: Border {
                radius: RADIUS.into(),
                ..Border::default()
            },
            ..container::Style::default()
        })
        .into()
    }
}

/// A display formula, scrolling sideways when it's wider than the notch.
fn display_math<'a>(tex: &str, t: Tokens) -> Element<'a, Message> {
    let ctx = Ctx {
        size: 17.0,
        display: true,
        bold: false,
        color: t.foreground,
    };
    scrollable(container(formula(&math::parse(tex), ctx)).padding([6, 12]))
        .direction(scrollable::Direction::Horizontal(
            scrollable::Scrollbar::new().width(4).scroller_width(4),
        ))
        .into()
}

#[derive(Clone, Copy)]
struct Ctx {
    size: f32,
    /// Display style: big operators take limits above and below.
    display: bool,
    bold: bool,
    color: Color,
}

impl Ctx {
    fn smaller(self, factor: f32) -> Self {
        Self {
            size: (self.size * factor).max(9.0),
            display: false,
            ..self
        }
    }

    fn glyphs<'a>(self, s: String) -> Element<'a, Message> {
        text(s)
            .size(self.size)
            .font(if self.bold { MATH_BOLD } else { MATH })
            .color(self.color)
            .into()
    }
}

/// Lays out a formula with native widgets: rows centred on the maths axis, stacked
/// fractions with a rule, raised and lowered scripts, matrices as aligned columns.
fn formula<'a>(node: &Node, ctx: Ctx) -> Element<'a, Message> {
    match node {
        Node::Row(items) => {
            let mut line = row![].align_y(Alignment::Center);
            for (index, item) in items.iter().enumerate() {
                let unary = matches!(item, Node::Op(_, Spacing::Binary))
                    && (index == 0
                        || matches!(&items[index - 1], Node::Op(s, sp)
                            if *sp != Spacing::None || matches!(s.as_str(), "(" | "[" | "{")));
                line = line.push(if unary {
                    let Node::Op(s, _) = item else { unreachable!() };
                    ctx.glyphs(s.clone())
                } else {
                    formula(item, ctx)
                });
                let named = matches!(item, Node::Text(s) | Node::Big(s, _)
                    if s.len() > 1 && s.chars().all(|c| c.is_ascii_alphabetic()));
                if named && matches!(items.get(index + 1), Some(Node::Ident(_) | Node::Number(_))) {
                    line = line.push(Space::new().width(ctx.size * 0.17));
                }
            }
            line.into()
        }
        Node::Ident(c) => ctx.glyphs(italic(*c).to_string()),
        Node::Number(n) => ctx.glyphs(n.clone()),
        Node::Text(s) => ctx.glyphs(s.clone()),
        Node::Op(s, spacing) => {
            let gap = ctx.size
                * match spacing {
                    Spacing::Binary => 0.22,
                    Spacing::Relation => 0.28,
                    Spacing::Punct | Spacing::None => 0.0,
                };
            let right = if *spacing == Spacing::Punct {
                ctx.size * 0.17
            } else {
                gap
            };
            container(ctx.glyphs(s.clone()))
                .padding(Padding {
                    left: gap,
                    right,
                    ..Padding::ZERO
                })
                .into()
        }
        Node::Bold(body) => formula(body, Ctx { bold: true, ..ctx }),
        Node::Big(s, _) => {
            let symbol = !s.chars().all(|c| c.is_ascii_alphabetic());
            let size = if symbol && ctx.display {
                ctx.size * 1.5
            } else {
                ctx.size
            };
            container(Ctx { size, ..ctx }.glyphs(s.clone()))
                .padding(Padding {
                    right: ctx.size * 0.12,
                    ..Padding::ZERO
                })
                .into()
        }
        Node::Frac(num, den) => {
            let part = ctx.smaller(if ctx.display { 0.9 } else { 0.8 });
            container(
                column![formula(num, part), bar(ctx), formula(den, part)]
                    .align_x(Alignment::Center)
                    .spacing(2)
                    .width(Length::Shrink),
            )
            .padding([0.0, ctx.size * 0.12])
            .into()
        }
        Node::Sqrt(index, body) => {
            let tall = contains_tall(body);
            let sign = Ctx {
                size: ctx.size * if tall { 1.7 } else { 1.1 },
                bold: false,
                ..ctx
            }
            .glyphs("√".into());
            let mut line = row![].align_y(Alignment::End);
            if let Some(index) = index {
                line = line.push(
                    column![
                        formula(index, ctx.smaller(0.55)),
                        Space::new().height(ctx.size * 0.5)
                    ]
                    .align_x(Alignment::End),
                );
            }
            line.push(sign)
                .push(
                    column![bar(ctx), formula(body, ctx)]
                        .spacing(2)
                        .width(Length::Shrink),
                )
                .into()
        }
        Node::Scripts { base, sub, sup } => {
            let script = ctx.smaller(0.7);
            if ctx.display && matches!(**base, Node::Big(_, Limits::Over)) {
                let mut stack = column![].align_x(Alignment::Center);
                if let Some(sup) = sup {
                    stack = stack.push(formula(sup, script));
                }
                stack = stack.push(formula(base, ctx));
                if let Some(sub) = sub {
                    stack = stack.push(formula(sub, script));
                }
                return container(stack)
                    .padding(Padding {
                        right: ctx.size * 0.15,
                        ..Padding::ZERO
                    })
                    .into();
            }
            let scripts: Element<'a, Message> = match (sup, sub) {
                (Some(sup), Some(sub)) => column![formula(sup, script), formula(sub, script)]
                    .spacing(ctx.size * 0.1)
                    .into(),
                (Some(sup), None) => {
                    column![formula(sup, script), Space::new().height(ctx.size * 0.75)].into()
                }
                (None, Some(sub)) => {
                    column![Space::new().height(ctx.size * 0.6), formula(sub, script)].into()
                }
                (None, None) => Space::new().into(),
            };
            row![formula(base, ctx), scripts]
                .align_y(Alignment::Center)
                .spacing(1)
                .into()
        }
        Node::Accent(mark, body) => match &**body {
            Node::Ident(c) => ctx.glyphs(format!("{}{mark}", italic(*c))),
            Node::Text(s) if s.chars().count() == 1 => ctx.glyphs(format!("{s}{mark}")),
            body => column![
                ctx.smaller(0.8).glyphs(spacing_mark(*mark).into()),
                formula(body, ctx)
            ]
            .align_x(Alignment::Center)
            .into(),
        },
        Node::Overline(body) => column![bar(ctx), formula(body, ctx)]
            .spacing(2)
            .width(Length::Shrink)
            .into(),
        Node::Matrix {
            rows,
            open,
            close,
            aligned,
        } => matrix(rows, open, close, *aligned, ctx),
        Node::Space(em) => Space::new().width(ctx.size * em).into(),
    }
}

fn matrix<'a>(
    rows: &[Vec<Node>],
    open: &str,
    close: &str,
    aligned: bool,
    ctx: Ctx,
) -> Element<'a, Message> {
    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    let cell_ctx = if aligned { ctx } else { ctx.smaller(0.95) };
    let mut grid = row![].align_y(Alignment::Start);
    for j in 0..width {
        let align = match (aligned, j % 2) {
            (true, 0) => Alignment::End,
            (true, _) => Alignment::Start,
            (false, _) => Alignment::Center,
        };
        let cells = rows.iter().map(|cells| match cells.get(j) {
            Some(cell) => formula(cell, cell_ctx),
            None => Space::new().into(),
        });
        let gap = match (aligned, j % 2) {
            (_, _) if j == 0 => 0.0,
            (true, 1) => 0.0,
            (true, _) => ctx.size * 1.5,
            (false, _) => ctx.size * 0.9,
        };
        grid = grid.push(Space::new().width(gap)).push(
            column(cells)
                .align_x(align)
                .spacing(ctx.size * 0.35)
                .width(Length::Shrink),
        );
    }
    let fence = |s: &str| {
        Ctx {
            size: ctx.size * (rows.len() as f32 * 1.1).max(1.0),
            bold: false,
            ..ctx
        }
        .glyphs(s.to_owned())
    };
    let mut line = row![].align_y(Alignment::Center).spacing(ctx.size * 0.15);
    if !open.is_empty() {
        line = line.push(fence(open));
    }
    line = line.push(grid);
    if !close.is_empty() {
        line = line.push(fence(close));
    }
    line.into()
}

/// The fraction / radical / overline rule, as wide as what it spans.
fn bar<'a>(ctx: Ctx) -> Element<'a, Message> {
    let color = ctx.color;
    container(Space::new())
        .width(Length::Fill)
        .height((ctx.size / 16.0).max(1.0))
        .style(move |_| container::Style {
            background: Some(Background::Color(color)),
            ..container::Style::default()
        })
        .into()
}

fn contains_tall(node: &Node) -> bool {
    match node {
        Node::Frac(..) | Node::Matrix { .. } => true,
        Node::Row(items) => items.iter().any(contains_tall),
        Node::Scripts { base, .. } => contains_tall(base),
        Node::Bold(b) | Node::Accent(_, b) | Node::Overline(b) | Node::Sqrt(_, b) => {
            contains_tall(b)
        }
        _ => false,
    }
}

/// Standalone forms of the combining accents, for accents over more than one symbol.
fn spacing_mark(mark: char) -> &'static str {
    match mark {
        '\u{302}' => "ˆ",
        '\u{304}' => "¯",
        '\u{303}' => "˜",
        '\u{20d7}' => "→",
        '\u{307}' => "˙",
        '\u{308}' => "¨",
        _ => "",
    }
}

/// Mathematical italic letters, as TeX sets variables. Upper-case Greek stays upright.
fn italic(c: char) -> char {
    let offset = |base: u32, from: char| char::from_u32(base + (c as u32 - from as u32));
    match c {
        'h' => Some('ℎ'),
        'a'..='z' => offset(0x1D44E, 'a'),
        'A'..='Z' => offset(0x1D434, 'A'),
        'α'..='ω' => offset(0x1D6FC, 'α'),
        _ => None,
    }
    .unwrap_or(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streaming_extends_the_parse_and_reparses_when_a_formula_closes() {
        let mut reply = Reply::new("Hello **wor");
        reply.update("Hello **world**\n\n- one\n- two");
        assert_eq!(reply.content.items().len(), 2);
        reply.update("Hello **world**\n\n- one\n- two\n\nArea $$A = ");
        let before = reply.content.items().len();
        reply.update("Hello **world**\n\n- one\n- two\n\nArea $$A = \\pi r^2$$");
        let after = reply.content.items();
        assert!(after.len() > before);
        assert!(after.iter().any(|item| matches!(
            item,
            markdown::Item::CodeBlock { language: Some(l), code, .. }
                if l == "math" && code.contains("\\pi r^2")
        )));
        // Same text: nothing to do.
        reply.update("Hello **world**\n\n- one\n- two\n\nArea $$A = \\pi r^2$$");
    }

    #[test]
    fn code_fences_keep_their_language() {
        let reply = Reply::new("```rust\nfn main() {}\n```");
        assert!(matches!(
            &reply.content.items()[0],
            markdown::Item::CodeBlock { language: Some(l), .. } if l == "rust"
        ));
    }

    #[test]
    fn italic_maps_letters() {
        assert_eq!(italic('x'), '𝑥');
        assert_eq!(italic('h'), 'ℎ');
        assert_eq!(italic('A'), '𝐴');
        assert_eq!(italic('π'), '𝜋');
        assert_eq!(italic('Γ'), 'Γ');
        assert_eq!(italic('1'), '1');
    }
}
