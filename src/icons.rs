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
    Palette,
    Sparkles,
    PanelTop,
    Cpu,
    Pencil,
    Trash,
    Plus,
    Star,
    Check,
    X,
    Copy,
    Square,
    ArrowUp,
    RefreshCw,
    MessagesSquare,
    Info,
    ChevronRight,
    Plug,
    Download,
    MessageSquarePlus,
    ClipboardPaste,
    RotateCcw,
    FolderOpen,
}

impl Icon {
    #[cfg(test)]
    pub const ALL: [Icon; 27] = [
        Icon::Sun,
        Icon::Moon,
        Icon::SlidersHorizontal,
        Icon::ChevronUp,
        Icon::Palette,
        Icon::Sparkles,
        Icon::PanelTop,
        Icon::Cpu,
        Icon::Pencil,
        Icon::Trash,
        Icon::Plus,
        Icon::Star,
        Icon::Check,
        Icon::X,
        Icon::Copy,
        Icon::Square,
        Icon::ArrowUp,
        Icon::RefreshCw,
        Icon::MessagesSquare,
        Icon::Info,
        Icon::ChevronRight,
        Icon::Plug,
        Icon::Download,
        Icon::MessageSquarePlus,
        Icon::ClipboardPaste,
        Icon::RotateCcw,
        Icon::FolderOpen,
    ];

    pub fn codepoint(self) -> char {
        match self {
            Icon::Sun => '\u{e178}',
            Icon::Moon => '\u{e11e}',
            Icon::SlidersHorizontal => '\u{e29a}',
            Icon::ChevronUp => '\u{e070}',
            Icon::Palette => '\u{e1dd}',
            Icon::Sparkles => '\u{e412}',
            Icon::PanelTop => '\u{e435}',
            Icon::Cpu => '\u{e0a9}',
            Icon::Pencil => '\u{e1f9}',
            Icon::Trash => '\u{e18e}',
            Icon::Plus => '\u{e13d}',
            Icon::Star => '\u{e176}',
            Icon::Check => '\u{e06c}',
            Icon::X => '\u{e1b2}',
            Icon::Copy => '\u{e09e}',
            Icon::Square => '\u{e167}',
            Icon::ArrowUp => '\u{e04a}',
            Icon::RefreshCw => '\u{e145}',
            Icon::MessagesSquare => '\u{e40d}',
            Icon::Info => '\u{e0f9}',
            Icon::ChevronRight => '\u{e06f}',
            Icon::Plug => '\u{e37f}',
            Icon::Download => '\u{e0b2}',
            Icon::MessageSquarePlus => '\u{e40c}',
            Icon::ClipboardPaste => '\u{e3e8}',
            Icon::RotateCcw => '\u{e148}',
            Icon::FolderOpen => '\u{e247}',
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

    /// Every glyph the enum names is in the subset, so none renders as a blank box.
    #[test]
    fn font_has_every_icon() {
        for icon in Icon::ALL {
            assert!(has_glyph(FONT_BYTES, icon.codepoint() as u16), "{icon:?}");
        }
        // Lucide's "activity", left out of the subset.
        assert!(!has_glyph(FONT_BYTES, 0xE038));
    }

    /// Looks `c` up in the font's Windows Unicode BMP cmap (format 4) subtable.
    fn has_glyph(font: &[u8], c: u16) -> bool {
        let u16_at = |at: usize| u16::from_be_bytes([font[at], font[at + 1]]);
        let u32_at = |at: usize| u32::from_be_bytes(font[at..at + 4].try_into().unwrap()) as usize;
        let cmap = (0..usize::from(u16_at(4)))
            .map(|i| 12 + 16 * i)
            .find(|&record| &font[record..record + 4] == b"cmap")
            .map(|record| u32_at(record + 8))
            .expect("cmap table");
        let table = (0..usize::from(u16_at(cmap + 2)))
            .map(|i| cmap + 4 + 8 * i)
            .find(|&record| u16_at(record) == 3 && u16_at(record + 2) == 1)
            .map(|record| cmap + u32_at(record + 4))
            .expect("Windows BMP subtable");
        assert_eq!(u16_at(table), 4, "cmap format");
        let segments = usize::from(u16_at(table + 6) / 2);
        let ends = table + 14;
        let starts = ends + 2 * segments + 2;
        let deltas = starts + 2 * segments;
        let offsets = deltas + 2 * segments;
        (0..segments).any(|i| {
            let (start, end) = (u16_at(starts + 2 * i), u16_at(ends + 2 * i));
            if !(start..=end).contains(&c) || c == 0xFFFF {
                return false;
            }
            let offset = usize::from(u16_at(offsets + 2 * i));
            let glyph = if offset == 0 {
                c.wrapping_add(u16_at(deltas + 2 * i))
            } else {
                let at = offsets + 2 * i + offset + 2 * usize::from(c - start);
                match u16_at(at) {
                    0 => 0,
                    g => g.wrapping_add(u16_at(deltas + 2 * i)),
                }
            };
            glyph != 0
        })
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
