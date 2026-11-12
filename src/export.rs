//! Markdown transcripts and portable, Windows-safe suggested file names.

use crate::model::Turn;

pub fn markdown(title: &str, turns: &[Turn]) -> String {
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    let title = if title.is_empty() {
        "bloop chat"
    } else {
        &title
    };
    let mut text = format!("# {title}\n\n");
    for turn in turns {
        text.push_str("**You**\n\n");
        text.push_str(&turn.user);
        text.push_str("\n\n**bloop**\n\n");
        // Put this before the content so an interrupted code fence cannot hide it.
        if !turn.complete {
            text.push_str("_(interrupted)_\n\n");
        }
        text.push_str(&turn.assistant);
        text.push_str("\n\n");
    }
    text
}

/// At most 120 UTF-16 code units including `.md`, without splitting Unicode characters.
pub fn file_name(title: &str) -> String {
    let clean: String = title
        .chars()
        .filter(|c| !c.is_control() && !"<>:\"/\\|?*".contains(*c))
        .collect();
    let clean = clean.trim().trim_end_matches('.').trim_end();
    let stem = if clean.to_ascii_lowercase().ends_with(".md") {
        &clean[..clean.len() - 3]
    } else {
        clean
    };
    let mut units = 0;
    let stem: String = stem
        .chars()
        .take_while(|c| {
            units += c.len_utf16();
            units <= 117
        })
        .collect();
    let stem = stem.trim_matches(|c: char| c.is_whitespace() || c == '.');
    if stem.is_empty() || reserved(stem) {
        "bloop chat.md".into()
    } else {
        format!("{stem}.md")
    }
}

fn reserved(stem: &str) -> bool {
    // Windows treats device names as reserved even when followed by an extension.
    let base = stem
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end()
        .to_ascii_uppercase();
    matches!(
        base.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || base
        .strip_prefix("COM")
        .or_else(|| base.strip_prefix("LPT"))
        .is_some_and(|suffix| {
            matches!(
                suffix,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcript_keeps_order_markdown_unicode_and_interrupted_text() {
        let turns = [
            Turn {
                user: "Explain 🦀".into(),
                assistant: "```rust\nlet x = 1;\n```".into(),
                complete: true,
            },
            Turn {
                user: "More?".into(),
                assistant: "A partial reply".into(),
                complete: false,
            },
        ];
        assert_eq!(
            markdown("  Rust\nchat  ", &turns),
            "# Rust chat\n\n**You**\n\nExplain 🦀\n\n**bloop**\n\n```rust\nlet x = 1;\n```\n\n**You**\n\nMore?\n\n**bloop**\n\n_(interrupted)_\n\nA partial reply\n\n"
        );
        assert_eq!(markdown("", &[]), "# bloop chat\n\n");
    }

    #[test]
    fn empty_interrupted_reply_is_still_marked() {
        let text = markdown(
            "Chat",
            &[Turn {
                user: "Hello".into(),
                assistant: String::new(),
                complete: false,
            }],
        );
        assert!(text.ends_with("**bloop**\n\n_(interrupted)_\n\n\n\n"));
    }

    #[test]
    fn file_names_strip_windows_special_characters_and_trim() {
        assert_eq!(file_name("  <A>: B/\\C|?*\"\u{0}.  "), "A BC.md");
        assert_eq!(file_name("Notes.MD"), "Notes.md");
        assert_eq!(file_name("Résumé 🦀"), "Résumé 🦀.md");
        for title in ["", "...  ", "<>:\"/\\|?*", " \n\t", ".md"] {
            assert_eq!(file_name(title), "bloop chat.md");
        }
    }

    #[test]
    fn device_names_are_rejected_even_with_extensions() {
        for title in [
            "CON", "nul.txt", "Aux.MD", "PRN", "com1", "LPT9.txt", "COM¹", "LPT²", "LPT³",
            "CONIN$", "CONOUT$", "CON .txt",
        ] {
            assert_eq!(file_name(title), "bloop chat.md", "{title}");
        }
        assert_eq!(file_name("company"), "company.md");
        assert_eq!(file_name("COM10"), "COM10.md");
    }

    #[test]
    fn file_name_length_is_bounded_in_utf16_without_splitting_characters() {
        for title in [
            "a".repeat(500),
            "🦀".repeat(100),
            format!("{} .xyz", "a".repeat(116)),
        ] {
            let name = file_name(&title);
            assert!(name.encode_utf16().count() <= 120);
            assert!(name.ends_with(".md"));
            assert!(!name.trim_end_matches(".md").ends_with([' ', '.']));
        }
    }
}
