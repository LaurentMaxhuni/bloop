//! The decorated app window and the windows' open/focus plumbing.

use super::{AppPage, Island, Message, notch_settings};
use crate::platform;
use bloop::secrets;
use iced::{Element, Length, Size, Task, widget::container, window};

/// Default and minimum size of the app window, in logical pixels.
const APP_SIZE: Size = Size::new(900.0, 620.0);
const APP_MIN_SIZE: Size = Size::new(720.0, 520.0);

/// A normal desktop window: decorated, resizable, centered and in the taskbar. Closing it
/// only hides bloop's window; the notch keeps running.
pub(super) fn app_settings() -> window::Settings {
    window::Settings {
        size: APP_SIZE,
        min_size: Some(APP_MIN_SIZE),
        position: window::Position::Centered,
        decorations: true,
        transparent: false,
        resizable: true,
        level: window::Level::Normal,
        exit_on_close_request: false,
        ..Default::default()
    }
}

impl Island {
    /// Brings the app window forward, opening it (with bloop's icon) if it is closed. A
    /// fresh window starts on Home with the drafts reset; focusing an open one keeps both.
    pub(super) fn open_app_window(&mut self) -> Task<Message> {
        if let Some(id) = self.app_window {
            return Task::batch([window::minimize(id, false), window::gain_focus(id)]);
        }
        self.page = AppPage::Home;
        self.draft_config = self.config.clone();
        self.key_draft.clear();
        self.key_status = secrets::key_source();
        self.autostart = platform::autostart();
        self.autostart_error.clear();
        let (id, open) = window::open(app_settings());
        self.app_window = Some(id);
        open.then(|id| window::run(id, platform::apply_app_icon))
            .discard()
    }

    /// Opens the notch window once; `Message::Window` places it when it exists.
    pub(super) fn open_notch(&mut self) -> Task<Message> {
        if self.window.is_some() {
            return Task::none();
        }
        let (id, open) = window::open(notch_settings());
        self.window = Some(id);
        open.map(|id| Message::Window(Some(id)))
    }

    /// The dashboard on its own opaque background, since the daemon's background stays
    /// transparent for the notch.
    pub(super) fn app_view(&self) -> Element<'_, Message> {
        let t = self.tokens();
        container(self.dashboard_view())
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_| container::Style {
                background: Some(t.background.into()),
                text_color: Some(t.foreground),
                ..container::Style::default()
            })
            .into()
    }
}
