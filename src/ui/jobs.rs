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
    async fn run_blocking(self, run: impl FnOnce(Self) -> Done + Send + 'static) -> Done {
        let chat = match &self {
            Job::Save { chat, .. } => chat.clone(),
            _ => None,
        };
        tokio::task::spawn_blocking(move || run(self))
            .await
            .unwrap_or_else(|error| Done::SaveFailed {
                chat,
                error: error.to_string(),
            })
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
    /// saves it makes redundant. A delete runs after writes already queued for its chat.
    pub(super) fn queue(&mut self, job: Job) -> Task<Message> {
        match &job {
            Job::Save { config, chat } => {
                let id = chat.as_ref().map(|(chat, _)| chat.id);
                let mut through_barrier = self.jobs.iter().rposition(|queued| {
                    matches!(queued, Job::Load(c) | Job::Export(c) | Job::Rename(c, _) | Job::Delete(c) if Some(*c) == id)
                }).map_or(0, |index| index + 1);
                self.jobs.retain(|queued| {
                    let may_coalesce = through_barrier == 0;
                    through_barrier = through_barrier.saturating_sub(1);
                    match queued {
                        Job::Save { chat: None, .. } => false,
                        Job::Save {
                            chat: Some((c, _)), ..
                        } if may_coalesce => Some(c.id) != id,
                        Job::Retry(c, _) if may_coalesce => Some(*c) != id,
                        _ => true,
                    }
                });
                // A previous failure may belong to a chat that is no longer active.
                for &failed_id in self.failed_saves.keys() {
                    if Some(failed_id) != id && !self.deleting.contains(&failed_id) && !self.jobs.iter().any(|job| {
                        matches!(job, Job::Save { chat: Some((c, _)), .. } if c.id == failed_id)
                            || matches!(job, Job::Delete(c) | Job::Retry(c, _) if *c == failed_id)
                    }) {
                        self.jobs.push_back(Job::Retry(failed_id, config.clone()));
                    }
                }
            }
            Job::Delete(id) => self
                .jobs
                .retain(|job| !matches!(job, Job::Load(c) if c == id)),
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
            job.run_blocking(move |job| job.run(&path)),
            Message::JobDone,
        )
    }

    fn drained(&mut self) -> Task<Message> {
        if self.quitting {
            if self.failed_saves.is_empty() {
                return iced::exit();
            }
            self.storage_warning =
                "Could not quit because some chats are unsaved. Quit again to retry saving.".into();
            self.install_after_save = false;
            return self.stop_quitting();
        }
        // A reply in progress defers the update again; the save after it ends drains here.
        if self.install_after_save && self.request.is_none() {
            self.install_after_save = false;
            return self.install_update();
        }
        Task::none()
    }

    fn stop_quitting(&mut self) -> Task<Message> {
        self.quitting = false;
        if self.is_open() {
            Task::none()
        } else {
            self.present(Event::Shortcut)
        }
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
                    follow_up = self.stop_quitting();
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
            Done::Deleted(id, result) => {
                self.deleting.remove(&id);
                match result {
                    Ok(()) => {
                        self.failed_saves.remove(&id);
                        self.forget_chat(id);
                        if self.chat.id == id {
                            self.start_chat(self.usable_profile(self.chat.profile));
                        }
                    }
                    Err(error) => self.chats_notice = format!("Couldn't delete the chat: {error}"),
                }
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_delete_during_quit_reopens_the_notch_with_a_warning() {
        let mut island = Island::new(bloop::launch::Launch::Manual).0;
        island.window = Some(iced::window::Id::unique());
        island.loading = false;
        island.db_path = Some("unused.sqlite3".into());
        let chat = Chat {
            id: 42,
            ..Chat::default()
        };
        let turns = vec![Turn {
            user: "question".into(),
            assistant: "unsaved reply".into(),
            complete: true,
        }];
        island.failed_saves.insert(chat.id, (chat, turns));
        island.deleting.insert(42);
        island.saving = true; // Delete is already running.

        let _ = island.update(Message::Quit);
        assert!(island.quitting);
        let _ = island.update(Message::JobDone(Done::Deleted(42, Err("disk full".into()))));
        let feedback = island.update(Message::JobDone(Done::Saved(None)));

        assert!(!island.quitting);
        assert!(island.is_open());
        assert!(island.storage_warning.contains("Could not quit"));
        assert!(island.failed_saves.contains_key(&42));
        assert!(feedback.units() > 0);
    }

    #[tokio::test]
    async fn save_worker_panic_keeps_the_outgoing_snapshot() {
        let mut island = Island::new(bloop::launch::Launch::Manual).0;
        let id = island.chat.id;
        let job = Job::Save {
            config: Box::new(Config::default()),
            chat: Some((
                island.chat.clone(),
                vec![Turn {
                    user: "question".into(),
                    assistant: "unsaved reply".into(),
                    complete: true,
                }],
            )),
        };
        island.start_chat(0);
        let done = job.run_blocking(|_| panic!("fixture worker panic")).await;
        let _ = island.job_done(done);

        assert_ne!(island.chat.id, id);
        assert_eq!(island.failed_saves[&id].1[0].assistant, "unsaved reply");
        assert!(island.storage_warning.contains("fixture worker panic"));
    }

    #[test]
    fn export_reads_the_save_queued_before_it() {
        let mut island = Island::new(bloop::launch::Launch::Manual).0;
        island.saving = true;
        let save = |answer: &str| Job::Save {
            config: Box::new(Config::default()),
            chat: Some((
                Chat {
                    id: 42,
                    ..Chat::default()
                },
                vec![Turn {
                    user: "prompt".into(),
                    assistant: answer.into(),
                    complete: true,
                }],
            )),
        };
        let _ = island.queue(save("original"));
        let _ = island.queue(Job::Export(42));
        let _ = island.queue(save("intermediate edit"));
        let _ = island.queue(save("final edit"));

        assert_eq!(island.jobs.len(), 3);
        assert!(
            matches!(&island.jobs[0], Job::Save { chat: Some((_, turns)), .. } if turns[0].assistant == "original")
        );
        assert!(matches!(&island.jobs[1], Job::Export(42)));
        assert!(
            matches!(&island.jobs[2], Job::Save { chat: Some((_, turns)), .. } if turns[0].assistant == "final edit")
        );
    }
}
