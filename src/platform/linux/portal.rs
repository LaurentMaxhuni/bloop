//! Desktop portals and helpers: system theme and reduced motion, the Save dialog, opening
//! links, and the startup-error dialog.

/// Shown when bloop can't start (no usable GPU driver).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub const FATAL: &str =
    "bloop could not start. A Vulkan or OpenGL-capable graphics driver is required.";

/// `org.freedesktop.appearance` `color-scheme`: 1 is dark; 0 (no preference), 2 and anything
/// else count as light.
pub fn color_scheme_is_light(value: u32) -> bool {
    value != 1
}

/// The portal's `reduced-motion` (1 = reduce) wins; else GNOME's `enable-animations`.
pub fn reduce_motion(portal: Option<u32>, gnome_animations: Option<bool>) -> bool {
    match portal {
        Some(value) => value == 1,
        None => gnome_animations == Some(false),
    }
}

/// `xdg-open <url>`: the URL is one argument, never parsed by a shell.
pub fn xdg_open_command(url: &str) -> std::process::Command {
    let mut command = std::process::Command::new("xdg-open");
    command.arg(url);
    command
}

#[cfg(target_os = "linux")]
mod io {
    use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

    /// 0 = not known yet (no portal), 1 = light, 2 = dark.
    static LIGHT: AtomicU8 = AtomicU8::new(0);
    static REDUCE: AtomicBool = AtomicBool::new(false);

    pub fn light_theme_system() -> bool {
        LIGHT.load(Ordering::Relaxed) == 1
    }

    pub fn reduce_motion_system() -> bool {
        REDUCE.load(Ordering::Relaxed)
    }

    /// Reads the theme and motion settings once, then follows changes, for the rest of the
    /// session. The UI reads the cached values; without a portal they keep their defaults.
    pub async fn watch_settings() {
        use ashpd::desktop::settings::Settings;
        use iced::futures::StreamExt as _;
        let Ok(settings) = Settings::new().await else {
            return;
        };
        let store_scheme = |value: u32| {
            let light = super::color_scheme_is_light(value);
            LIGHT.store(if light { 1 } else { 2 }, Ordering::Relaxed);
        };
        if let Ok(value) = settings
            .read::<u32>("org.freedesktop.appearance", "color-scheme")
            .await
        {
            store_scheme(value);
        }
        let portal_motion = settings
            .read::<u32>("org.freedesktop.appearance", "reduced-motion")
            .await
            .ok();
        let gnome = settings
            .read::<bool>("org.gnome.desktop.interface", "enable-animations")
            .await
            .ok();
        REDUCE.store(
            super::reduce_motion(portal_motion, gnome),
            Ordering::Relaxed,
        );
        let Ok(mut changes) = settings.receive_setting_changed().await else {
            return;
        };
        while let Some(change) = changes.next().await {
            match (change.namespace(), change.key()) {
                ("org.freedesktop.appearance", "color-scheme") => {
                    if let Ok(value) = u32::try_from(change.value()) {
                        store_scheme(value);
                    }
                }
                ("org.freedesktop.appearance", "reduced-motion") => {
                    if let Ok(value) = u32::try_from(change.value()) {
                        REDUCE.store(value == 1, Ordering::Relaxed);
                    }
                }
                _ => {}
            }
        }
    }

    /// The portal's Save dialog. Call from a blocking worker (it waits for the user).
    pub fn save_file_dialog(suggested_name: &str) -> Result<Option<std::path::PathBuf>, String> {
        use ashpd::desktop::file_chooser::SaveFileRequest;
        let handle = tokio::runtime::Handle::try_current()
            .map_err(|_| "No file dialog is available on this desktop.".to_owned())?;
        let name = suggested_name.to_owned();
        handle.block_on(async move {
            let request = SaveFileRequest::default()
                .title("Export chat")
                .current_name(name.as_str())
                .modal(true)
                .send()
                .await
                .map_err(|_| "No file dialog is available on this desktop.".to_owned())?;
            match request.response() {
                Ok(files) => Ok(files.uris().first().and_then(|uri| uri.to_file_path().ok())),
                Err(ashpd::Error::Response(ashpd::desktop::ResponseError::Cancelled)) => Ok(None),
                Err(error) => Err(format!("The Save dialog failed: {error}")),
            }
        })
    }

    /// Opens a link with the desktop's default handler, without a shell.
    pub fn open_url(url: &str) -> Result<(), String> {
        let mut command = super::xdg_open_command(url);
        super::super::child_env(&mut command);
        let mut child = command
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|_| "Couldn't open the link.".to_owned())?;
        // Reap it so no zombie is left behind.
        std::thread::spawn(move || child.wait());
        Ok(())
    }

    /// Startup error: log, stderr, and a dialog if zenity or kdialog is installed.
    pub fn show_fatal(detail: &str) {
        let message = format!("{}\n\n{detail}", super::FATAL);
        eprintln!("{message}");
        let shown = std::process::Command::new("zenity")
            .args(["--error", "--title", "bloop — startup error", "--text"])
            .arg(&message)
            .status()
            .is_ok_and(|status| status.success() || status.code() == Some(1));
        if !shown {
            let _ = std::process::Command::new("kdialog")
                .args(["--title", "bloop — startup error", "--error"])
                .arg(&message)
                .status();
        }
    }
}

#[cfg(target_os = "linux")]
pub use io::{
    light_theme_system, open_url, reduce_motion_system, save_file_dialog, show_fatal,
    watch_settings,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_scheme_values() {
        assert!(!color_scheme_is_light(1), "prefer dark");
        assert!(color_scheme_is_light(2), "prefer light");
        assert!(color_scheme_is_light(0), "no preference");
        assert!(color_scheme_is_light(7), "unknown");
    }

    #[test]
    fn reduce_motion_prefers_the_portal_setting() {
        assert!(reduce_motion(Some(1), Some(true)));
        assert!(!reduce_motion(Some(0), Some(false)));
        assert!(reduce_motion(None, Some(false)), "GNOME animations off");
        assert!(!reduce_motion(None, None));
    }

    #[test]
    fn xdg_open_takes_url_as_one_argument() {
        let url = "https://example.com/a b;rm -rf ~";
        let command = xdg_open_command(url);
        assert_eq!(command.get_program(), "xdg-open");
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(args, [url]);
    }
}
