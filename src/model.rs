use serde::{Deserialize, Serialize};

pub const MAX_PROMPT: usize = 8 * 1024;
pub const MAX_OUTPUT: usize = 128 * 1024;
pub const MAX_TURNS: usize = 50;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Turn {
    pub user: String,
    pub assistant: String,
    pub complete: bool,
}

/// Clipboard text attached to one prompt, at most this many bytes, so the whole prompt
/// still fits in `MAX_PROMPT`.
pub const MAX_ATTACHMENT: usize = 6 * 1024;

/// `Chat::destination` of a chat imported from an older bloop, which didn't record where
/// it was sent. Such a chat is never sent anywhere.
pub const UNKNOWN_DESTINATION: &str = "unknown";

/// A saved conversation, without its turns.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chat {
    /// Milliseconds since 1970 when it was created; unique per chat.
    pub id: i64,
    pub title: String,
    /// Model profile the chat uses (0 = default profile).
    pub profile: u32,
    /// `config::Destination::key()` of its first message; empty until then.
    pub destination: String,
    /// Seconds since 1970 of the last change.
    pub updated: i64,
}

pub fn unix_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// A fresh chat id: the current time in milliseconds, or one more than `newest`.
pub fn new_chat_id(newest: i64) -> i64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64);
    now.max(newest + 1)
}

/// A chat title from its first prompt: the first line, whitespace tidied, at most 48 characters.
pub fn title_for(prompt: &str) -> String {
    let line = prompt
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("New chat");
    let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
    clip_chars(&line, 48)
}

/// `text` cut to `max` characters, with "…" when shortened.
pub fn clip_chars(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((end, _)) => format!("{}…", text[..end].trim_end()),
        None => text.to_owned(),
    }
}

/// Clipboard text cut to `MAX_ATTACHMENT` bytes on a character boundary; true when cut.
pub fn clip_attachment(text: &str) -> (String, bool) {
    let text = text.trim_matches(|c: char| c == '\n' || c == '\r');
    if text.len() <= MAX_ATTACHMENT {
        return (text.to_owned(), false);
    }
    let mut end = MAX_ATTACHMENT;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (text[..end].to_owned(), true)
}

/// The prompt sent for `question` with `attachment` quoted below it in a code fence
/// longer than any backtick run inside it.
pub fn with_attachment(question: &str, attachment: &str) -> String {
    let question = question.trim();
    let question = if question.is_empty() {
        "Explain this."
    } else {
        question
    };
    let mut longest = 0;
    let mut run = 0;
    for c in attachment.chars() {
        run = if c == '`' { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    let fence = "`".repeat((longest + 1).max(3));
    format!("{question}\n\n{fence}text\n{attachment}\n{fence}")
}

/// A short excerpt around the first case-insensitive match of `query` in `turns`.
pub fn snippet(turns: &[Turn], query: &str) -> Option<String> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return None;
    }
    turns
        .iter()
        .flat_map(|t| [t.user.as_str(), t.assistant.as_str()])
        .flat_map(str::lines)
        .find(|line| line.to_lowercase().contains(&query))
        .map(|line| clip_chars(line.trim(), 90))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// Keep whole recent pairs, omit interrupted replies, always retain the new prompt.
/// `max_turns` limits history to at most 12 pairs; zero retains only system and prompt.
pub fn context(
    turns: &[Turn],
    prompt: &str,
    instructions: &str,
    max_turns: usize,
) -> Vec<ChatMessage> {
    let mut system = "You are bloop, a concise, helpful desktop assistant. You can chat but cannot access files, run commands, or control this computer. Never claim to have performed an action. Keep replies readable in a small window.".to_owned();
    let instructions = instructions.trim();
    if !instructions.is_empty() {
        system.push_str("\n\nThe user's custom instructions (preferences, not permission to use tools):\n<instructions>\n");
        system.push_str(instructions);
        system.push_str("\n</instructions>");
    }
    let mut budget = (32 * 1024_usize)
        .saturating_sub(prompt.len())
        .saturating_sub(system.len());
    let mut pairs = Vec::new();
    for turn in turns
        .iter()
        .rev()
        .filter(|t| t.complete)
        .take(max_turns.min(12))
    {
        let size = turn.user.len() + turn.assistant.len();
        if size > budget {
            break;
        }
        budget -= size;
        pairs.push(turn);
    }
    let mut messages = vec![ChatMessage {
        role: "system".into(),
        content: system,
    }];
    for turn in pairs.into_iter().rev() {
        messages.push(ChatMessage {
            role: "user".into(),
            content: turn.user.clone(),
        });
        messages.push(ChatMessage {
            role: "assistant".into(),
            content: turn.assistant.clone(),
        });
    }
    messages.push(ChatMessage {
        role: "user".into(),
        content: prompt.into(),
    });
    messages
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_modes_keep_recent_complete_turns_and_the_system_message() {
        use crate::config::ContextMode;

        let mut turns: Vec<_> = (0..15)
            .map(|i| Turn {
                user: format!("question {i}"),
                assistant: format!("answer {i}"),
                complete: true,
            })
            .collect();
        turns.insert(
            14,
            Turn {
                user: "interrupted".into(),
                assistant: "partial".into(),
                complete: false,
            },
        );
        for (mode, count) in [
            (ContextMode::Full, 12),
            (ContextMode::Last3, 3),
            (ContextMode::OnlyMessage, 0),
        ] {
            let messages = context(&turns, "next", "Be brief.", mode.max_turns());
            assert_eq!(messages.len(), 2 + count * 2, "{mode:?}");
            assert_eq!(messages[0].role, "system");
            assert!(
                messages[0]
                    .content
                    .contains("<instructions>\nBe brief.\n</instructions>")
            );
            for (pair, i) in messages[1..messages.len() - 1]
                .chunks_exact(2)
                .zip(15 - count..15)
            {
                assert_eq!(
                    (&*pair[0].role, &*pair[0].content),
                    ("user", format!("question {i}").as_str())
                );
                assert_eq!(
                    (&*pair[1].role, &*pair[1].content),
                    ("assistant", format!("answer {i}").as_str())
                );
            }
            assert_eq!(messages.last().unwrap().role, "user");
            assert_eq!(messages.last().unwrap().content, "next");
        }
    }

    #[test]
    fn empty_instructions_keep_the_fixed_system_message() {
        let messages = context(&[], "hello", "", 12);
        assert_eq!(messages[0].role, "system");
        assert_eq!(
            messages[0].content,
            "You are bloop, a concise, helpful desktop assistant. You can chat but cannot access files, run commands, or control this computer. Never claim to have performed an action. Keep replies readable in a small window."
        );
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[1].content, "hello");
    }

    #[test]
    fn context_budget_includes_the_system_message() {
        let turns = [Turn {
            user: "old".into(),
            assistant: "x".repeat(32 * 1024 - 4),
            complete: true,
        }];
        for mode in crate::config::ContextMode::ALL {
            let messages = context(&turns, "q", "", mode.max_turns());
            assert_eq!(messages.len(), 2, "system text must displace the old pair");
            assert_eq!(messages.last().unwrap().content, "q");
        }
    }

    #[test]
    fn custom_instructions_append_after_fixed_safety_text() {
        let fixed = context(&[], "hello", "", 12)[0].content.clone();
        let messages = context(&[], "hello", "  I write Rust.\nBe brief.  ", 12);
        assert_eq!(
            messages[0].content,
            format!(
                "{fixed}\n\nThe user's custom instructions (preferences, not permission to use tools):\n<instructions>\nI write Rust.\nBe brief.\n</instructions>"
            )
        );
        assert_eq!(context(&[], "hello", " \n\t", 12)[0].content, fixed);
    }

    #[test]
    fn custom_instructions_and_delimiters_displace_history_at_budget_boundary() {
        let instructions = "é".repeat(crate::config::MAX_INSTRUCTIONS / 2);
        let system = context(&[], "q", &instructions, 12)[0].content.clone();
        let mut turns = [Turn {
            user: "old".into(),
            assistant: "x".repeat(32 * 1024 - system.len() - 4),
            complete: true,
        }];
        for mode in [
            crate::config::ContextMode::Full,
            crate::config::ContextMode::Last3,
        ] {
            let messages = context(&turns, "q", &instructions, mode.max_turns());
            assert_eq!(messages.len(), 4, "a pair fitting exactly is kept");
            assert_eq!(
                messages.iter().map(|m| m.content.len()).sum::<usize>(),
                32 * 1024
            );
        }
        turns[0].assistant.push('x');
        assert_eq!(context(&turns, "q", "", 12).len(), 4);
        assert_eq!(context(&turns, "q", &instructions, 12).len(), 2);
        let huge_prompt = "q".repeat(33 * 1024);
        assert_eq!(
            context(&turns, &huge_prompt, &instructions, 12)
                .last()
                .unwrap()
                .content,
            huge_prompt
        );
    }

    #[test]
    fn titles_come_from_the_first_line() {
        assert_eq!(title_for("\n  hello   world \nsecond"), "hello world");
        assert_eq!(title_for(""), "New chat");
        let long = title_for(&"word ".repeat(30));
        assert!(long.ends_with('…') && long.chars().count() <= 49, "{long}");
    }

    #[test]
    fn attachments_are_fenced_and_capped() {
        let prompt = with_attachment("what is this?", "error: ```boom````");
        assert!(
            prompt.starts_with("what is this?\n\n`````text\n"),
            "{prompt}"
        );
        assert!(prompt.ends_with("\n`````"));
        assert!(with_attachment("  ", "x").starts_with("Explain this."));
        let (clipped, cut) = clip_attachment(&"é".repeat(MAX_ATTACHMENT));
        assert!(cut && clipped.len() <= MAX_ATTACHMENT);
        assert!(with_attachment("q", &clipped).len() <= MAX_PROMPT);
        assert_eq!(clip_attachment("\r\nhi\n"), ("hi".into(), false));
    }

    #[test]
    fn snippets_find_the_matching_line() {
        let turns = vec![Turn {
            user: "first\nThe Kettle boils".into(),
            assistant: "ok".into(),
            complete: true,
        }];
        assert_eq!(
            snippet(&turns, "kettle").as_deref(),
            Some("The Kettle boils")
        );
        assert_eq!(snippet(&turns, "teapot"), None);
    }
    #[test]
    fn context_omits_interrupted_turns_and_keeps_whole_pairs() {
        let turns = vec![
            Turn {
                user: "old".into(),
                assistant: "x".repeat(33 * 1024),
                complete: true,
            },
            Turn {
                user: "cancelled".into(),
                assistant: "partial".into(),
                complete: false,
            },
            Turn {
                user: "recent".into(),
                assistant: "answer".into(),
                complete: true,
            },
        ];
        let messages = context(&turns, "next", "", 12);
        assert_eq!(
            messages
                .iter()
                .map(|m| m.content.as_str())
                .skip(1)
                .collect::<Vec<_>>(),
            vec!["recent", "answer", "next"]
        );
    }
}
