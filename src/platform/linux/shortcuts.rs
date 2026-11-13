//! Global shortcuts on Linux: X11 key grabs on X11 sessions, the GlobalShortcuts portal on
//! Wayland desktops that offer it, and nothing (with a warning) elsewhere.

use super::session::Session;

/// Which shortcut mechanism this session gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    X11,
    Portal,
    None,
}

/// X11 sessions grab keys directly; Wayland sessions need the GlobalShortcuts portal.
pub fn backend(session: Session, portal_available: bool) -> Backend {
    match session {
        Session::X11 => Backend::X11,
        Session::XWayland | Session::WaylandOnly if portal_available => Backend::Portal,
        Session::XWayland | Session::WaylandOnly => Backend::None,
    }
}

/// bloop's shortcut text (`Ctrl+Shift+Space`) as a portal trigger (`CTRL+SHIFT+space`):
/// upper-case `CTRL` / `ALT` / `SHIFT` / `LOGO` modifiers and an XKB key name.
pub fn portal_trigger(shortcut: &str) -> Option<String> {
    let parts: Vec<&str> = shortcut.split('+').map(str::trim).collect();
    let (key, modifiers) = parts.split_last()?;
    if key.is_empty() {
        return None;
    }
    let mut out = Vec::new();
    for modifier in modifiers {
        out.push(match modifier.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => "CTRL",
            "alt" | "option" => "ALT",
            "shift" => "SHIFT",
            "super" | "meta" | "win" | "cmd" | "command" | "logo" => "LOGO",
            _ => return None,
        });
    }
    let key = if key.chars().count() == 1 {
        key.to_ascii_lowercase()
    } else if key.eq_ignore_ascii_case("space") {
        "space".into()
    } else {
        (*key).to_owned()
    };
    Some(
        out.into_iter()
            .chain([key.as_str()])
            .collect::<Vec<_>>()
            .join("+"),
    )
}

/// A shortcut change from the UI (see `platform::Controller`).
#[derive(Debug, Clone)]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub enum Command {
    Shortcut(String),
    AskShortcut(String),
}

/// Shown when neither X11 key grabs nor the shortcuts portal are available.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub const NO_SHORTCUTS: &str = "This desktop has no global shortcut service. Bind \"bloop --toggle\" and \"bloop --ask\" to keys in your keyboard settings.";

#[cfg(target_os = "linux")]
mod run {
    use super::{Backend, Command, NO_SHORTCUTS, backend, portal_trigger};
    use crate::platform::{Event, Shortcuts};
    use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut};
    use iced::futures::StreamExt as _;
    use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

    /// Applies shortcut commands for the rest of the session.
    pub async fn run(sender: UnboundedSender<Event>, inbox: UnboundedReceiver<Command>) {
        let session = super::super::session();
        let portal = match backend(session, true) {
            Backend::Portal => portal().await,
            _ => None,
        };
        match backend(session, portal.is_some()) {
            Backend::X11 => {
                std::thread::spawn(move || x11(sender, inbox));
            }
            Backend::Portal => {
                if let Some(portal) = portal {
                    portal_loop(portal, sender, inbox).await;
                }
            }
            Backend::None => none(sender, inbox).await,
        }
    }

    async fn portal() -> Option<(GlobalShortcuts, ashpd::desktop::Session<GlobalShortcuts>)> {
        let proxy = GlobalShortcuts::new().await.ok()?;
        let session = proxy.create_session(Default::default()).await.ok()?;
        Some((proxy, session))
    }

    async fn none(sender: UnboundedSender<Event>, mut inbox: UnboundedReceiver<Command>) {
        while inbox.recv().await.is_some() {
            let _ = sender.send(Event::Warning(NO_SHORTCUTS.into()));
        }
    }

    /// X11: grab keys with `global-hotkey`, on its own thread like Windows.
    fn x11(sender: UnboundedSender<Event>, mut inbox: UnboundedReceiver<Command>) {
        use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
        let manager = match GlobalHotKeyManager::new() {
            Ok(manager) => manager,
            Err(error) => {
                let _ = sender.send(Event::Warning(format!(
                    "Global shortcut unavailable: {error}"
                )));
                return;
            }
        };
        let shortcuts = std::sync::Arc::new(std::sync::Mutex::new(Shortcuts::default()));
        let hotkey_shortcuts = shortcuts.clone();
        let hotkey_sender = sender.clone();
        GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
            if event.state == HotKeyState::Pressed
                && let Some(action) = hotkey_shortcuts.lock().unwrap().event(event.id)
            {
                let _ = hotkey_sender.send(action);
            }
        }));
        while let Some(command) = inbox.blocking_recv() {
            let (slot, value, label) = match command {
                Command::Shortcut(value) => (0, value, "Global shortcut"),
                Command::AskShortcut(value) => (1, value, "Ask about clipboard shortcut"),
            };
            let result = shortcuts.lock().unwrap().apply(slot, &value, |add, key| {
                if add {
                    manager.register(key)
                } else {
                    manager.unregister(key)
                }
                .map_err(|e| e.to_string())
            });
            let _ = sender.send(match result {
                Ok(()) => Event::ShortcutApplied,
                Err(error) => Event::Warning(format!("{label} unavailable: {error}")),
            });
        }
    }

    /// Wayland: bind both shortcuts through the portal. The desktop may show its own dialog and
    /// pick a different key combination; bloop shows what it reports.
    async fn portal_loop(
        (proxy, session): (GlobalShortcuts, ashpd::desktop::Session<GlobalShortcuts>),
        sender: UnboundedSender<Event>,
        mut inbox: UnboundedReceiver<Command>,
    ) {
        let activated_sender = sender.clone();
        if let Ok(mut activated) = proxy.receive_activated().await {
            tokio::spawn(async move {
                while let Some(event) = activated.next().await {
                    let action = match event.shortcut_id() {
                        "toggle" => Event::Toggle,
                        "ask" => Event::Ask,
                        _ => continue,
                    };
                    if activated_sender.send(action).is_err() {
                        break;
                    }
                }
            });
        }
        let (mut toggle, mut ask) = (String::new(), String::new());
        while let Some(command) = inbox.recv().await {
            let mut apply = |command| match command {
                Command::Shortcut(value) => toggle = value,
                Command::AskShortcut(value) => ask = value,
            };
            apply(command);
            // The UI sends both settings together: bind once for the pair.
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            while let Ok(command) = inbox.try_recv() {
                apply(command);
            }
            let mut wanted = vec![
                NewShortcut::new("toggle", "Show or collapse bloop")
                    .preferred_trigger(portal_trigger(&toggle).as_deref()),
            ];
            if !ask.is_empty() {
                wanted.push(
                    NewShortcut::new("ask", "Ask bloop about the clipboard")
                        .preferred_trigger(portal_trigger(&ask).as_deref()),
                );
            }
            let bound = async {
                proxy
                    .bind_shortcuts(&session, &wanted, None, Default::default())
                    .await?
                    .response()
            }
            .await;
            match bound {
                Ok(bound) => {
                    let list = bound
                        .shortcuts()
                        .iter()
                        .map(|s| (s.id().to_owned(), s.trigger_description().to_owned()))
                        .collect();
                    let _ = sender.send(Event::ShortcutsManagedBySystem(list));
                    let _ = sender.send(Event::ShortcutApplied);
                }
                Err(error) => {
                    let _ = sender.send(Event::Warning(format!(
                        "Global shortcut unavailable: {error}"
                    )));
                }
            }
        }
    }
}

#[cfg(target_os = "linux")]
pub use run::run;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portal_trigger_maps_modifiers() {
        assert_eq!(
            portal_trigger("Ctrl+Shift+Space").as_deref(),
            Some("CTRL+SHIFT+space")
        );
        assert_eq!(portal_trigger("Alt+K").as_deref(), Some("ALT+k"));
        assert_eq!(portal_trigger("Super+F5").as_deref(), Some("LOGO+F5"));
        assert_eq!(portal_trigger("Ctrl+Alt+B").as_deref(), Some("CTRL+ALT+b"));
        assert_eq!(portal_trigger(""), None);
        assert_eq!(portal_trigger("Ctrl+"), None);
    }

    #[test]
    fn backend_choice() {
        assert_eq!(backend(Session::X11, false), Backend::X11);
        assert_eq!(backend(Session::X11, true), Backend::X11);
        assert_eq!(backend(Session::XWayland, true), Backend::Portal);
        assert_eq!(backend(Session::XWayland, false), Backend::None);
        assert_eq!(backend(Session::WaylandOnly, true), Backend::Portal);
    }
}
