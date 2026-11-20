//! Every database write, and every read of a whole chat, runs through one queue, one job at
//! a time, in the order asked. Each job carries a snapshot taken when it was queued, so
//! switching chats never loses the outgoing chat's save, a rename or delete can't be undone
//! by an older save landing later, and opening a chat sees its newest saved turns.

use super::{Island, Message};
use crate::presence::Event;
use bloop::{
    config::Config,
    model::{Chat, Turn},
    storage,
};
use iced::Task;

#[derive(Debug, Clone)]
pub enum Job {
    /// Settings, and the given chat when it has turns.
    Save {
        config: Box<Config>,
        chat: Option<(Chat, Vec<Turn>)>,
    },
    Retry(i64, Box<Config>),
    Rename(i64, String),
    Delete(i64),
    /// Read a chat to show it in the notch.
    Load(i64),
    /// Read a chat to export it.
    Export(i64),
}

#[derive(Debug, Clone)]
pub enum Done {
    Saved(Option<i64>),
    SaveFailed {
        chat: Option<(Chat, Vec<Turn>)>,
        error: String,
    },
    Renamed(i64, String, Result<(), String>),
    Deleted(i64, Result<(), String>),
    Loaded(i64, Result<(Chat, Vec<Turn>), String>),
    ForExport(i64, Result<(Chat, Vec<Turn>), String>),
}

impl Job {
    /// Whether the job only touches chat `id`, so deleting that chat makes it pointless.
    fn only_for_chat(&self, id: i64) -> bool {
        matches!(self, Job::Retry(c, _) | Job::Rename(c, _) | Job::Load(c) | Job::Export(c) if *c == id)
    }

    fn run(self, path: &std::path::Path) -> Done {
        match self {
            Job::Save { config, chat } => {
                match storage::save(
                    path,
                    &config,
                    chat.as_ref().map(|(chat, turns)| (chat, turns.as_slice())),
                ) {
                    Ok(()) => Done::Saved(chat.map(|(chat, _)| chat.id)),
                    Err(error) => Done::SaveFailed { chat, error },
                }
            }
            Job::Rename(id, title) => {
                let result = storage::rename_chat(path, id, &title);
                Done::Renamed(id, title, result)
            }
            Job::Delete(id) => Done::Deleted(id, storage::delete_chat(path, id)),
            Job::Load(id) => Done::Loaded(id, storage::load_chat(path, id)),
            Job::Export(id) => Done::ForExport(id, storage::load_chat(path, id)),
            Job::Retry(..) => unreachable!("retries resolve before running"),
        }
    }
}

impl Island {
    /// Adds `job` to the queue and starts it if nothing is running. A save replaces queued
    /// saves it makes redundant. A delete drops queued renames and reads of that chat, and
    /// turns queued saves of it into settings-only saves so their settings still land.
    pub(super) fn queue(&mut self, job: Job) -> Task<Message> {
        match &job {
            Job::Save { config, chat } => {
                let id = chat.as_ref().map(|(chat, _)| chat.id);
                self.jobs.retain(|queued| match queued {
                    Job::Save { chat: None, .. } => false,
                    Job::Save {
                        chat: Some((c, _)), ..
                    } => Some(c.id) != id,
                    Job::Retry(c, _) => Some(*c) != id,
                    _ => true,
                });
                // A previous failure may belong to a chat that is no longer active.
                for &failed_id in self.failed_saves.keys() {
                    if Some(failed_id) != id && !self.jobs.iter().any(|job| {
                        matches!(job, Job::Save { chat: Some((c, _)), .. } if c.id == failed_id)
                            || matches!(job, Job::Delete(c) | Job::Retry(c, _) if *c == failed_id)
                    }) {
                        self.jobs.push_back(Job::Retry(failed_id, config.clone()));
                    }
                }
            }
            Job::Delete(id) => {
                self.jobs.retain(|queued| !queued.only_for_chat(*id));
                for queued in &mut self.jobs {
                    if let Job::Save { chat, .. } = queued
                        && chat.as_ref().is_some_and(|(c, _)| c.id == *id)
                    {
                        *chat = None;
                    }
                }
            }
            Job::Retry(..) | Job::Rename(..) | Job::Load(_) | Job::Export(_) => {}
        }
        self.jobs.push_back(job);
        self.next_job()
    }

    /// Starts the next queued job, or finishes a quit or update once the queue is empty.
    fn next_job(&mut self) -> Task<Message> {
        if self.saving {
            return Task::none();
        }
        let Some(path) = self.db_path.clone() else {
            self.jobs.clear();
            return self.drained();
        };
        let job = loop {
            match self.jobs.pop_front() {
                Some(Job::Retry(id, config)) => {
                    if let Some(chat) = self.failed_saves.get(&id).cloned() {
                        break Job::Save {
                            config,
                            chat: Some(chat),
                        };
                    }
                }
                Some(job) => break job,
                None => return self.drained(),
            }
        };
        self.saving = true;
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || job.run(&path))
                    .await
                    .unwrap_or_else(|e| Done::SaveFailed {
                        chat: None,
                        error: e.to_string(),
                    })
            },
            Message::JobDone,
        )
    }

    fn drained(&mut self) -> Task<Message> {
        if self.quitting {
            return iced::exit();
        }
        // A reply in progress defers the update again; the save after it ends drains here.
        if self.install_after_save && self.request.is_none() {
            self.install_after_save = false;
            return self.install_update();
        }
        Task::none()
    }

    /// Whether every queued write has finished.
    pub(super) fn idle_storage(&self) -> bool {
        !self.saving && self.jobs.is_empty()
    }

    pub(super) fn job_done(&mut self, done: Done) -> Task<Message> {
        self.saving = false;
        let mut follow_up = Task::none();
        match done {
            Done::Saved(id) => {
                if let Some(id) = id {
                    self.failed_saves.remove(&id);
                }
                if self.failed_saves.is_empty() {
                    self.storage_warning.clear();
                }
            }
            Done::SaveFailed { chat, error } => {
                if let Some(snapshot) = chat {
                    self.failed_saves.insert(snapshot.0.id, snapshot);
                }
                self.storage_warning = format!("Could not save: {error}");
                self.install_after_save = false;
                if self.quitting {
                    // Stay open so nothing is lost; the warning offers quitting anyway.
                    self.quitting = false;
                    if !self.is_open() {
                        follow_up = self.present(Event::Shortcut);
                    }
                }
            }
            Done::Renamed(id, title, result) => match result {
                Ok(()) => {
                    if let Some((chat, _)) = self.failed_saves.get_mut(&id) {
                        chat.title = title;
                    }
                }
                Err(error) => self.chats_notice = format!("Couldn't rename the chat: {error}"),
            },
            Done::Deleted(id, result) => match result {
                Ok(()) => {
                    self.failed_saves.remove(&id);
                    self.forget_chat(id);
                }
                Err(error) => self.chats_notice = format!("Couldn't delete the chat: {error}"),
            },
            Done::Loaded(id, result) => {
                let result = self.failed_saves.get(&id).cloned().map_or(result, Ok);
                follow_up = self.opened_chat(id, result);
            }
            Done::ForExport(id, result) => {
                let result = self.failed_saves.get(&id).cloned().map_or(result, Ok);
                follow_up = self.export_loaded(result);
            }
        }
        Task::batch([follow_up, self.next_job()])
    }
}
