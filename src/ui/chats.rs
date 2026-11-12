//! The Chats page: every saved conversation, with search, open, rename, export and delete.
//! Only the current chat is written by `persist`; the others change here directly.

use super::{Island, Message};
use crate::{
    platform,
    presence::Event,
    style::{self, MEDIUM, Variant},
};
use bloop::{
    config::Destination,
    export,
    model::{self, Chat, Turn},
    storage,
};
use iced::{
    Alignment, Element, Length, Task,
    widget::{self, Space, column, container, row, text},
};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum ChatAction {
    Search(String),
    Results(u64, Result<Vec<(Chat, String)>, String>),
    /// Show this chat in the notch.
    Open(i64),
    Opened(Result<(Chat, Vec<Turn>), String>),
    Rename(i64),
    RenameInput(String),
    RenameSave,
    RenameCancel,
    Renamed(Result<(), String>),
    Delete(i64),
    DeleteConfirm,
    DeleteCancel,
    Deleted(i64, Result<(), String>),
    Export(i64),
    Exported(Result<Option<PathBuf>, String>),
}

/// Runs `job` on a blocking thread with the database path.
fn blocking<T: Send + 'static>(
    path: PathBuf,
    job: impl FnOnce(&std::path::Path) -> Result<T, String> + Send + 'static,
) -> impl Future<Output = Result<T, String>> {
    async move {
        tokio::task::spawn_blocking(move || job(&path))
            .await
            .map_err(|e| e.to_string())?
    }
}

/// "Just now", "5 min ago", "3 h ago", "Yesterday", "4 days ago", "6 weeks ago".
pub fn when(updated: i64, now: i64) -> String {
    let seconds = (now - updated).max(0);
    let (minutes, hours, days) = (seconds / 60, seconds / 3600, seconds / 86_400);
    match () {
        _ if minutes < 1 => "Just now".into(),
        _ if hours < 1 => format!("{minutes} min ago"),
        _ if days < 1 => format!("{hours} h ago"),
        _ if days < 2 => "Yesterday".into(),
        _ if days < 14 => format!("{days} days ago"),
        _ => format!("{} weeks ago", days / 7),
    }
}

impl Island {
    fn chat_store(&mut self) -> Option<PathBuf> {
        if self.storage_disabled || self.loading {
            self.chats_notice = "Chats can't be changed while storage is unavailable.".into();
            return None;
        }
        self.db_path.clone()
    }

    pub(super) fn chat_action(&mut self, action: ChatAction) -> Task<Message> {
        match action {
            ChatAction::Search(query) => {
                self.chat_query = query;
                self.search_id += 1;
                if self.chat_query.trim().is_empty() {
                    self.chat_hits = None;
                    return Task::none();
                }
                let Some(path) = self.db_path.clone() else {
                    return Task::none();
                };
                let (id, query) = (self.search_id, self.chat_query.clone());
                return Task::perform(
                    blocking(path, move |path| storage::search(path, &query)),
                    move |result| Message::Chat(ChatAction::Results(id, result)),
                );
            }
            ChatAction::Results(id, result) => {
                if id == self.search_id {
                    match result {
                        Ok(hits) => self.chat_hits = Some(hits),
                        Err(error) => self.chats_notice = format!("Search failed: {error}"),
                    }
                }
            }
            ChatAction::Open(id) => {
                if self.request.is_some() {
                    self.chats_notice = "Stop the reply in the notch before switching chats.".into();
                    return Task::none();
                }
                if id == self.chat.id {
                    return self.show_notch();
                }
                let Some(path) = self.db_path.clone() else {
                    return Task::none();
                };
                return Task::perform(
                    blocking(path, move |path| storage::load_chat(path, id)),
                    |result| Message::Chat(ChatAction::Opened(result)),
                );
            }
            ChatAction::Opened(result) => match result {
                Ok((chat, turns)) if self.request.is_none() => {
                    self.start_chat(chat.profile);
                    self.chat = chat;
                    self.turns = turns;
                    self.sync_replies();
                    self.chats_notice.clear();
                    let scroll = self.scroll();
                    return Task::batch([self.show_notch(), scroll]);
                }
                Ok(_) => {}
                Err(error) => self.chats_notice = format!("Couldn't open that chat: {error}"),
            },
            ChatAction::Rename(id) => {
                let title = self
                    .chats
                    .iter()
                    .find(|c| c.id == id)
                    .map(|c| c.title.clone())
                    .unwrap_or_default();
                self.renaming = Some((id, title));
                return widget::operation::focus("rename");
            }
            ChatAction::RenameInput(value) => {
                if let Some((_, title)) = &mut self.renaming
                    && value.len() <= 200
                {
                    *title = value;
                }
            }
            ChatAction::RenameCancel => self.renaming = None,
            ChatAction::RenameSave => {
                let Some((id, title)) = self.renaming.clone() else {
                    return Task::none();
                };
                let title = title.trim().to_owned();
                if title.is_empty() {
                    self.chats_notice = "Give the chat a name.".into();
                    return Task::none();
                }
                let Some(path) = self.chat_store() else {
                    return Task::none();
                };
                self.renaming = None;
                self.chats_notice.clear();
                for chat in self.chats.iter_mut().chain(
                    self.chat_hits
                        .iter_mut()
                        .flat_map(|hits| hits.iter_mut().map(|(chat, _)| chat)),
                ) {
                    if chat.id == id {
                        chat.title = title.clone();
                    }
                }
                if id == self.chat.id {
                    // The current chat is written with its title on every save.
                    self.chat.title = title;
                    return self.persist();
                }
                return Task::perform(
                    blocking(path, move |path| storage::rename_chat(path, id, &title)),
                    |result| Message::Chat(ChatAction::Renamed(result)),
                );
            }
            ChatAction::Renamed(result) => {
                if let Err(error) = result {
                    self.chats_notice = format!("Couldn't rename the chat: {error}");
                }
            }
            ChatAction::Delete(id) => self.confirm_delete = Some(id),
            ChatAction::DeleteCancel => self.confirm_delete = None,
            ChatAction::DeleteConfirm => {
                let Some(id) = self.confirm_delete.take() else {
                    return Task::none();
                };
                if self.chat_store().is_none() {
                    return Task::none();
                }
                if id == self.chat.id {
                    if self.request.is_some() {
                        self.chats_notice = "Stop the reply before deleting this chat.".into();
                        return Task::none();
                    }
                    let profile = self.usable_profile(self.chat.profile);
                    self.start_chat(profile);
                    if self.saving {
                        // A save of this chat is in flight; delete it once that lands.
                        self.forget_chat(id);
                        self.delete_after_save = Some(id);
                        return Task::none();
                    }
                }
                return self.delete_chat(id);
            }
            ChatAction::Deleted(id, result) => match result {
                Ok(()) => self.forget_chat(id),
                Err(error) => self.chats_notice = format!("Couldn't delete the chat: {error}"),
            },
            ChatAction::Export(id) => {
                let Some(path) = self.db_path.clone() else {
                    return Task::none();
                };
                let current = (id == self.chat.id).then(|| (self.chat.clone(), self.turns.clone()));
                return Task::perform(
                    blocking(path, move |path| {
                        let (chat, turns) = match current {
                            Some(current) => current,
                            None => storage::load_chat(path, id)?,
                        };
                        let Some(target) =
                            platform::save_file_dialog(&export::file_name(&chat.title))?
                        else {
                            return Ok(None);
                        };
                        std::fs::write(&target, export::markdown(&chat.title, &turns))
                            .map_err(|e| e.to_string())?;
                        Ok(Some(target))
                    }),
                    |result| Message::Chat(ChatAction::Exported(result)),
                );
            }
            ChatAction::Exported(result) => match result {
                Ok(Some(target)) => {
                    self.chats_notice = format!("Saved {}", target.display());
                }
                Ok(None) => {}
                Err(error) => self.chats_notice = format!("Couldn't export the chat: {error}"),
            },
        }
        Task::none()
    }

    /// Deletes a chat that isn't being saved right now.
    pub(super) fn delete_chat(&mut self, id: i64) -> Task<Message> {
        let Some(path) = self.db_path.clone() else {
            return Task::none();
        };
        self.forget_chat(id);
        Task::perform(
            blocking(path, move |path| storage::delete_chat(path, id)),
            move |result| Message::Chat(ChatAction::Deleted(id, result)),
        )
    }

    fn forget_chat(&mut self, id: i64) {
        self.chats.retain(|c| c.id != id);
        if let Some(hits) = &mut self.chat_hits {
            hits.retain(|(c, _)| c.id != id);
        }
    }

    /// Opens the notch panel (from the app window) without toggling it shut.
    fn show_notch(&mut self) -> Task<Message> {
        if self.is_open() || self.window.is_none() {
            return Task::none();
        }
        self.present(Event::Shortcut)
    }

    pub(super) fn chats_page(&self) -> Element<'_, Message> {
        let t = self.tokens();
        let search = self
            .input("Search chats", &self.chat_query, |q| {
                Message::Chat(ChatAction::Search(q))
            })
            .id("chat-search");
        let mut page = column![search].spacing(14);
        if !self.chats_notice.is_empty() {
            page = page.push(text(&self.chats_notice).size(13).color(t.muted_foreground));
        }
        if self.storage_disabled {
            page = page.push(
                text("Storage is unavailable, so chats aren't being saved.")
                    .size(13)
                    .color(t.destructive),
            );
        }
        let all: Vec<(&Chat, &str)>;
        let rows: &[(&Chat, &str)] = match &self.chat_hits {
            Some(hits) => {
                all = hits.iter().map(|(c, s)| (c, s.as_str())).collect();
                &all
            }
            None => {
                all = self.chats.iter().map(|c| (c, "")).collect();
                &all
            }
        };
        if rows.is_empty() {
            let empty = if self.chat_hits.is_some() {
                "No chats match."
            } else {
                "No saved chats yet. Chats appear here after their first reply."
            };
            page = page.push(text(empty).size(13).color(t.muted_foreground));
        }
        let now = model::unix_seconds();
        for (chat, snippet) in rows {
            page = page.push(self.chat_row(chat, snippet, now));
        }
        page.into()
    }

    fn chat_row<'a>(&'a self, chat: &'a Chat, snippet: &'a str, now: i64) -> Element<'a, Message> {
        let t = self.tokens();
        let current = chat.id == self.chat.id;
        let title: Element<'a, Message> = match &self.renaming {
            Some((id, draft)) if *id == chat.id => row![
                self.input("Chat name", draft, |v| Message::Chat(ChatAction::RenameInput(v)))
                    .id("rename")
                    .on_submit(Message::Chat(ChatAction::RenameSave)),
                self.button("Save", Variant::Primary)
                    .on_press(Message::Chat(ChatAction::RenameSave)),
                self.button("Cancel", Variant::Ghost)
                    .on_press(Message::Chat(ChatAction::RenameCancel)),
            ]
            .spacing(6)
            .align_y(Alignment::Center)
            .into(),
            _ => text(if chat.title.is_empty() {
                "Untitled chat"
            } else {
                &chat.title
            })
            .size(14)
            .font(MEDIUM)
            .into(),
        };
        let mut meta = vec![when(chat.updated, now)];
        if !chat.destination.is_empty() {
            meta.push(Destination::from_key(&chat.destination).label().to_owned());
        }
        if let Some(name) = self.config.profile_name(chat.profile)
            && chat.profile != 0
        {
            meta.push(name.to_owned());
        }
        if current {
            meta.push("In the notch".into());
        }
        let mut details = column![
            title,
            text(meta.join(" · ")).size(12).color(t.muted_foreground)
        ]
        .spacing(4);
        if !snippet.is_empty() {
            details = details.push(text(snippet).size(12).color(t.muted_foreground));
        }
        let mut card = column![details].spacing(10);
        if self.confirm_delete == Some(chat.id) {
            card = card.push(
                row![
                    text("Delete this chat from this computer? This can't be undone.")
                        .size(13)
                        .width(Length::Fill),
                    self.button("Keep", Variant::Outline)
                        .on_press(Message::Chat(ChatAction::DeleteCancel)),
                    self.button("Delete", Variant::Destructive)
                        .on_press(Message::Chat(ChatAction::DeleteConfirm)),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            );
        } else {
            let idle = self.request.is_none();
            card = card.push(
                row![
                    self.button("Open", Variant::Outline)
                        .on_press_maybe(idle.then_some(Message::Chat(ChatAction::Open(chat.id)))),
                    self.button("Rename", Variant::Ghost)
                        .on_press(Message::Chat(ChatAction::Rename(chat.id))),
                    self.button("Export", Variant::Ghost)
                        .on_press(Message::Chat(ChatAction::Export(chat.id))),
                    Space::new().width(Length::Fill),
                    self.button("Delete", Variant::Ghost)
                        .on_press(Message::Chat(ChatAction::Delete(chat.id))),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            );
        }
        container(card)
            .padding([12, 14])
            .width(Length::Fill)
            .style(style::card(t, t.background))
            .into()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn relative_times() {
        let now = 10_000_000;
        assert_eq!(super::when(now - 5, now), "Just now");
        assert_eq!(super::when(now - 300, now), "5 min ago");
        assert_eq!(super::when(now - 3 * 3600, now), "3 h ago");
        assert_eq!(super::when(now - 30 * 3600, now), "Yesterday");
        assert_eq!(super::when(now - 4 * 86_400, now), "4 days ago");
        assert_eq!(super::when(now - 42 * 86_400, now), "6 weeks ago");
        assert_eq!(super::when(now + 50, now), "Just now");
    }
}
