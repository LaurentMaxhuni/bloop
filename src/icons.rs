//! Lucide icons (ISC licence, `assets/LUCIDE-LICENSE.txt`) from lucide-static 1.61.0, cut
//! down to the glyphs bloop uses with `fonttools subset`. To add an icon, look up its code
//! point in that version's `font/lucide.css` and re-subset with every code point below.

use iced::Font;

pub const FONT_BYTES: &[u8] = include_bytes!("../assets/lucide-icons.ttf");
pub const FONT: Font = Font::with_name("lucide");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Sun,
    Moon,
    SlidersHorizontal,
    ChevronUp,
    ArrowLeft,
    Palette,
    Sparkles,
    PanelTop,
    Cpu,
}

impl Icon {
    pub const ALL: [Icon; 9] = [
        Icon::Sun,
        Icon::Moon,
        Icon::SlidersHorizontal,
        Icon::ChevronUp,
        Icon::ArrowLeft,
        Icon::Palette,
        Icon::Sparkles,
        Icon::PanelTop,
        Icon::Cpu,
    ];

    pub fn codepoint(self) -> char {
        match self {
            Icon::Sun => '\u{e178}',
            Icon::Moon => '\u{e11e}',
            Icon::SlidersHorizontal => '\u{e29a}',
            Icon::ChevronUp => '\u{e070}',
            Icon::ArrowLeft => '\u{e048}',
            Icon::Palette => '\u{e1dd}',
            Icon::Sparkles => '\u{e412}',
            Icon::PanelTop => '\u{e435}',
            Icon::Cpu => '\u{e0a9}',
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codepoints_are_private_use() {
        for icon in Icon::ALL {
            let c = icon.codepoint() as u32;
            assert!((0xE000..=0xF8FF).contains(&c), "{icon:?} {c:x}");
        }
    }

    #[test]
    fn font_bytes_look_like_ttf() {
        assert!(
            FONT_BYTES.starts_with(&[0x00, 0x01, 0x00, 0x00]) || FONT_BYTES.starts_with(b"true")
        );
        assert!(
            FONT_BYTES.len() < 64 * 1024,
            "subset font grew to {} bytes",
            FONT_BYTES.len()
        );
    }
}
