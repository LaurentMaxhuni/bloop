//! Which display backend bloop uses on Linux. bloop draws through X11 everywhere (XWayland on
//! Wayland desktops), so the notch can stay on top and place itself. Processes bloop starts get
//! the original `WAYLAND_DISPLAY` back, so browsers and CLIs still run as native Wayland apps.

use std::ffi::OsString;
use std::sync::OnceLock;

/// The kind of desktop session bloop is drawing in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Session {
    /// A plain X11 desktop.
    X11,
    /// A Wayland desktop; bloop draws through XWayland.
    XWayland,
    /// Wayland without XWayland (or `BLOOP_BACKEND=wayland`): the notch can't keep itself on top.
    WaylandOnly,
}

/// What `main` does to the environment before any thread starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backend {
    pub session: Session,
    /// Remove `WAYLAND_DISPLAY` so winit picks X11.
    pub drop_wayland_display: bool,
    /// The removed value, given back to processes bloop starts.
    pub original_wayland_display: Option<OsString>,
}

/// Decides the backend from environment variables (pure).
pub fn choose_backend(var: impl Fn(&str) -> Option<OsString>) -> Backend {
    let present = |name: &str| var(name).is_some_and(|value| !value.is_empty());
    let wayland_display = var("WAYLAND_DISPLAY").filter(|value| !value.is_empty());
    let wayland = wayland_display.is_some()
        || var("XDG_SESSION_TYPE").is_some_and(|value| value == "wayland");
    let forced_wayland = var("BLOOP_BACKEND").is_some_and(|value| value == "wayland");
    if !present("DISPLAY") || (forced_wayland && wayland) {
        return Backend {
            session: if wayland {
                Session::WaylandOnly
            } else {
                Session::X11
            },
            drop_wayland_display: false,
            original_wayland_display: None,
        };
    }
    Backend {
        session: if wayland {
            Session::XWayland
        } else {
            Session::X11
        },
        drop_wayland_display: wayland_display.is_some(),
        original_wayland_display: wayland_display,
    }
}

static BACKEND: OnceLock<Backend> = OnceLock::new();

/// Picks the backend and removes `WAYLAND_DISPLAY` when drawing through XWayland.
/// Must be the first thing `main` does, before any thread exists.
#[cfg(target_os = "linux")]
pub fn init_backend() {
    let backend = choose_backend(|name| std::env::var_os(name));
    if backend.drop_wayland_display {
        // SAFETY: called first thing in `main`, before tracing, Tokio or Iced start a thread,
        // so nothing can read the environment concurrently.
        unsafe { std::env::remove_var("WAYLAND_DISPLAY") };
    }
    let _ = BACKEND.set(backend);
}

/// The session `init_backend` found; `X11` if it never ran (tests).
#[allow(dead_code)]
pub fn session() -> Session {
    BACKEND
        .get()
        .map_or(Session::X11, |backend| backend.session)
}

fn apply_child_env(backend: &Backend, command: &mut std::process::Command) {
    if let Some(value) = &backend.original_wayland_display {
        command.env("WAYLAND_DISPLAY", value);
    }
}

/// Gives a process bloop starts the original `WAYLAND_DISPLAY` back.
#[allow(dead_code)]
pub fn child_env(command: &mut std::process::Command) {
    if let Some(backend) = BACKEND.get() {
        apply_child_env(backend, command);
    }
}

/// [`child_env`] for Tokio commands.
#[allow(dead_code)]
pub fn child_env_tokio(command: &mut tokio::process::Command) {
    if let Some(value) = BACKEND
        .get()
        .and_then(|backend| backend.original_wayland_display.as_ref())
    {
        command.env("WAYLAND_DISPLAY", value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |key| {
            pairs
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| value.into())
        }
    }

    #[test]
    fn x11_session_keeps_everything() {
        let backend = choose_backend(env(&[("DISPLAY", ":0"), ("XDG_SESSION_TYPE", "x11")]));
        assert_eq!(backend.session, Session::X11);
        assert!(!backend.drop_wayland_display);
        assert_eq!(backend.original_wayland_display, None);
    }

    #[test]
    fn wayland_with_xwayland_drops_wayland_display() {
        let backend = choose_backend(env(&[("DISPLAY", ":0"), ("WAYLAND_DISPLAY", "wayland-0")]));
        assert_eq!(backend.session, Session::XWayland);
        assert!(backend.drop_wayland_display);
        assert_eq!(
            backend.original_wayland_display.as_deref(),
            Some(OsStr::new("wayland-0"))
        );
    }

    #[test]
    fn wayland_session_type_counts_as_xwayland() {
        let backend = choose_backend(env(&[("DISPLAY", ":0"), ("XDG_SESSION_TYPE", "wayland")]));
        assert_eq!(backend.session, Session::XWayland);
        assert!(!backend.drop_wayland_display, "nothing to drop");
    }

    #[test]
    fn wayland_without_display_stays_wayland() {
        let backend = choose_backend(env(&[("WAYLAND_DISPLAY", "wayland-0")]));
        assert_eq!(backend.session, Session::WaylandOnly);
        assert!(!backend.drop_wayland_display);
    }

    #[test]
    fn backend_override_keeps_wayland() {
        let backend = choose_backend(env(&[
            ("DISPLAY", ":0"),
            ("WAYLAND_DISPLAY", "w"),
            ("BLOOP_BACKEND", "wayland"),
        ]));
        assert_eq!(backend.session, Session::WaylandOnly);
        assert!(!backend.drop_wayland_display);
    }

    #[test]
    fn child_env_restores_wayland_display() {
        let backend = choose_backend(env(&[("DISPLAY", ":0"), ("WAYLAND_DISPLAY", "wayland-0")]));
        let mut command = std::process::Command::new("true");
        apply_child_env(&backend, &mut command);
        assert!(
            command
                .get_envs()
                .any(|(k, v)| k == "WAYLAND_DISPLAY" && v == Some(OsStr::new("wayland-0")))
        );
        let x11 = choose_backend(env(&[("DISPLAY", ":0")]));
        let mut command = std::process::Command::new("true");
        apply_child_env(&x11, &mut command);
        assert_eq!(command.get_envs().count(), 0);
    }
}
