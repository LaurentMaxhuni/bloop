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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// Keep whole recent pairs, omit interrupted replies, always retain the new prompt.
pub fn context(turns: &[Turn], prompt: &str) -> Vec<ChatMessage> {
    let mut budget = 32 * 1024_usize - prompt.len().min(32 * 1024);
    let mut pairs = Vec::new();
    for turn in turns.iter().rev().filter(|t| t.complete).take(12) {
        let size = turn.user.len() + turn.assistant.len();
        if size > budget {
            break;
        }
        budget -= size;
        pairs.push(turn);
    }
    let mut messages = vec![ChatMessage {
        role: "system".into(),
        content: "You are bloop, a concise, helpful desktop assistant. You can chat but cannot access files, run commands, or control this computer. Never claim to have performed an action. Keep replies readable in a small window.".into(),
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
        let messages = context(&turns, "next");
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
