//! Desktop integration files and tiling-desktop window rules.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The app ID portals know bloop by; the launcher and autostart files are named after it.
pub const APP_ID: &str = "io.github.LaurentMaxhuni.bloop";

/// An `Exec=` value running `exe` with `args`: the program quoted (escaping `"`, `` ` ``, `$`
/// and `\`), `%` doubled, and every backslash doubled again for the file format.
pub fn exec_line(exe: &Path, args: &[&str]) -> String {
    let mut quoted = String::from("\"");
    for c in exe.to_string_lossy().chars() {
        match c {
            '"' | '`' | '$' | '\\' => {
                quoted.push('\\');
                quoted.push(c);
            }
            '%' => quoted.push_str("%%"),
            _ => quoted.push(c),
        }
    }
    quoted.push('"');
    let mut line = quoted.replace('\\', "\\\\");
    for arg in args {
        line.push(' ');
        line.push_str(arg);
    }
    line
}

/// The program an `Exec=` line runs: the inverse of [`exec_line`] for its first argument.
pub fn exec_target(entry: &str) -> Option<PathBuf> {
    let value = entry.lines().find_map(|line| line.strip_prefix("Exec="))?;
    let value = value.replace("\\\\", "\\");
    let mut chars = value.chars();
    if chars.next()? != '"' {
        return value.split(' ').next().map(PathBuf::from);
    }
    let mut out = String::new();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.push(chars.next()?),
            '"' => return Some(PathBuf::from(out.replace("%%", "%"))),
            _ => out.push(c),
        }
    }
    None
}

/// A launcher (`background: false`) or autostart (`background: true`) entry for `exe`.
pub fn desktop_entry(exe: &Path, background: bool) -> String {
    let args: &[&str] = if background { &["--background"] } else { &[] };
    let mut entry = format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=bloop\n\
         Comment=A local-first AI notch\n\
         Exec={}\n\
         Icon=bloop\n\
         Terminal=false\n\
         Categories=Utility;\n\
         StartupWMClass=bloop\n",
        exec_line(exe, args)
    );
    if background {
        entry.push_str("X-GNOME-Autostart-enabled=true\n");
    }
    entry
}

/// False when the entry is switched off (`Hidden=true`).
pub fn autostart_enabled(entry: &str) -> bool {
    !entry.lines().any(|line| line.trim() == "Hidden=true")
}

/// The autostart entry points at a program that no longer exists (the AppImage moved), so it
/// should point here instead. An entry naming another copy that still exists is left alone.
pub fn autostart_is_stale(entry: &str, exe: &Path, exists: impl Fn(&Path) -> bool) -> bool {
    exec_target(entry).is_some_and(|target| target != exe && !exists(&target))
}

fn xdg_dir(var: &impl Fn(&str) -> Option<OsString>, name: &str, fallback: &str) -> Option<PathBuf> {
    var(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            var("HOME")
                .filter(|h| !h.is_empty())
                .map(|home| PathBuf::from(home).join(fallback))
        })
}

pub fn autostart_path(var: &impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    Some(
        xdg_dir(var, "XDG_CONFIG_HOME", ".config")?
            .join("autostart")
            .join(format!("{APP_ID}.desktop")),
    )
}

pub fn launcher_path(var: &impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    Some(
        xdg_dir(var, "XDG_DATA_HOME", ".local/share")?
            .join("applications")
            .join(format!("{APP_ID}.desktop")),
    )
}

pub fn icon_path(var: &impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    Some(
        xdg_dir(var, "XDG_DATA_HOME", ".local/share")?.join("icons/hicolor/256x256/apps/bloop.png"),
    )
}

/// Tiling window managers that need a rule to float the notch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tiling {
    Hyprland,
    Sway,
    I3,
}

/// Which tiling desktop this is, from its environment variables (pure).
pub fn tiling(var: impl Fn(&str) -> Option<OsString>) -> Option<Tiling> {
    let set = |name: &str| var(name).is_some_and(|value| !value.is_empty());
    if set("HYPRLAND_INSTANCE_SIGNATURE") {
        return Some(Tiling::Hyprland);
    }
    if set("SWAYSOCK") {
        return Some(Tiling::Sway);
    }
    if set("I3SOCK") {
        return Some(Tiling::I3);
    }
    let desktop = var("XDG_CURRENT_DESKTOP")?
        .to_string_lossy()
        .to_ascii_lowercase();
    desktop.split(':').find_map(|name| match name {
        "hyprland" => Some(Tiling::Hyprland),
        "sway" => Some(Tiling::Sway),
        "i3" => Some(Tiling::I3),
        _ => None,
    })
}

/// The config line that floats and pins the notch (matched by its window title).
pub fn rule(kind: Tiling) -> &'static str {
    match kind {
        Tiling::Hyprland => "windowrulev2 = float, pin, noborder, noshadow, title:^(bloop notch)$",
        Tiling::Sway | Tiling::I3 => {
            "for_window [title=\"^bloop notch$\"] floating enable, sticky enable, border none"
        }
    }
}

/// Where the rule goes, for the card's text.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn config_file(kind: Tiling) -> &'static str {
    match kind {
        Tiling::Hyprland => "~/.config/hypr/hyprland.conf",
        Tiling::Sway => "~/.config/sway/config",
        Tiling::I3 => "~/.config/i3/config",
    }
}

/// The program the desktop should start: the AppImage file when running as one.
#[cfg(target_os = "linux")]
pub fn current_exe_for_desktop() -> Option<PathBuf> {
    std::env::var_os("APPIMAGE")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::current_exe().ok())
}

#[cfg(target_os = "linux")]
fn env_var(name: &str) -> Option<OsString> {
    std::env::var_os(name)
}

#[cfg(target_os = "linux")]
fn write_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, bytes)
}

/// Start on login is on: the autostart entry exists and isn't hidden.
#[cfg(target_os = "linux")]
pub fn autostart() -> bool {
    autostart_path(&env_var)
        .and_then(|path| std::fs::read_to_string(path).ok())
        .is_some_and(|entry| autostart_enabled(&entry))
}

#[cfg(target_os = "linux")]
pub fn set_autostart(enable: bool) -> Result<(), String> {
    let path = autostart_path(&env_var).ok_or("Could not find your config folder.")?;
    if enable {
        let exe = current_exe_for_desktop().ok_or("Could not find bloop's program file.")?;
        write_file(&path, desktop_entry(&exe, true).as_bytes())
            .map_err(|e| format!("Could not turn on start on login: {e}"))
    } else {
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("Could not turn off start on login: {error}")),
        }
    }
}

/// Points the autostart entry here when the AppImage it named has moved away.
#[cfg(target_os = "linux")]
pub fn migrate_autostart() {
    let (Some(path), Some(exe)) = (autostart_path(&env_var), current_exe_for_desktop()) else {
        return;
    };
    let Ok(entry) = std::fs::read_to_string(&path) else {
        return;
    };
    if autostart_enabled(&entry)
        && autostart_is_stale(&entry, &exe, Path::exists)
        && let Err(error) = write_file(&path, desktop_entry(&exe, true).as_bytes())
    {
        tracing::warn!("could not migrate the autostart entry: {error}");
    }
}

/// Installs the launcher entry and icon (bloop's app ID for portals, and a menu entry), and
/// rewrites them when the program moved.
#[cfg(target_os = "linux")]
pub fn ensure_launcher_entry() {
    let Some(exe) = current_exe_for_desktop() else {
        return;
    };
    if let Some(path) = launcher_path(&env_var) {
        let wanted = desktop_entry(&exe, false);
        if std::fs::read_to_string(&path).ok().as_deref() != Some(wanted.as_str()) {
            if let Err(error) = write_file(&path, wanted.as_bytes()) {
                tracing::warn!("could not write the launcher entry: {error}");
            } else if let Some(dir) = path.parent() {
                // Best effort: refresh menus that cache entries.
                let _ = std::process::Command::new("update-desktop-database")
                    .arg(dir)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn();
            }
        }
    }
    if let Some(icon) = icon_path(&env_var)
        && std::fs::read(&icon).ok().as_deref() != Some(super::icon::PNG)
        && let Err(error) = write_file(&icon, super::icon::PNG)
    {
        tracing::warn!("could not install the icon: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |key| {
            pairs
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| value.into())
        }
    }

    #[test]
    fn tiling_detection() {
        assert_eq!(
            tiling(env(&[("HYPRLAND_INSTANCE_SIGNATURE", "x")])),
            Some(Tiling::Hyprland)
        );
        assert_eq!(
            tiling(env(&[("SWAYSOCK", "/run/sway.sock")])),
            Some(Tiling::Sway)
        );
        assert_eq!(tiling(env(&[("I3SOCK", "/run/i3")])), Some(Tiling::I3));
        assert_eq!(
            tiling(env(&[("XDG_CURRENT_DESKTOP", "Hyprland")])),
            Some(Tiling::Hyprland)
        );
        assert_eq!(
            tiling(env(&[("XDG_CURRENT_DESKTOP", "sway")])),
            Some(Tiling::Sway)
        );
        assert_eq!(
            tiling(env(&[("XDG_CURRENT_DESKTOP", "ubuntu:GNOME")])),
            None
        );
        assert_eq!(tiling(env(&[])), None);
    }

    #[test]
    fn exec_line_quotes_spaces_and_percent() {
        assert_eq!(
            exec_line(
                Path::new("/home/a b/100%/bloop.AppImage"),
                &["--background"]
            ),
            "\"/home/a b/100%%/bloop.AppImage\" --background"
        );
        assert_eq!(
            exec_line(Path::new("/opt/x\"y$z`w/bloop"), &[]),
            // Quoting escapes `"`, `$` and `` ` `` with a backslash; the file format then
            // doubles every backslash.
            r#""/opt/x\\"y\\$z\\`w/bloop""#
        );
    }

    #[test]
    fn exec_target_reads_back_the_program() {
        let entry = desktop_entry(Path::new("/home/a b/100%/bloop.AppImage"), true);
        assert_eq!(
            exec_target(&entry).as_deref(),
            Some(Path::new("/home/a b/100%/bloop.AppImage"))
        );
        let tricky = desktop_entry(Path::new("/opt/x\"y$z/bloop"), false);
        assert_eq!(
            exec_target(&tricky).as_deref(),
            Some(Path::new("/opt/x\"y$z/bloop"))
        );
        assert_eq!(
            exec_target(
                "[Desktop Entry]
Name=x
"
            ),
            None
        );
    }

    #[test]
    fn desktop_entry_fields() {
        let entry = desktop_entry(Path::new("/apps/bloop"), true);
        for line in [
            "[Desktop Entry]",
            "Type=Application",
            "Name=bloop",
            "Icon=bloop",
            "StartupWMClass=bloop",
            "Exec=\"/apps/bloop\" --background",
            "X-GNOME-Autostart-enabled=true",
        ] {
            assert!(
                entry.lines().any(|l| l == line),
                "missing {line}
{entry}"
            );
        }
        let launcher = desktop_entry(Path::new("/apps/bloop"), false);
        assert!(launcher.lines().any(|l| l == "Exec=\"/apps/bloop\""));
        assert!(!launcher.contains("Autostart"));
    }

    #[test]
    fn migration_detects_moved_appimage() {
        let old = desktop_entry(Path::new("/old/bloop.AppImage"), true);
        let here = Path::new("/new/bloop.AppImage");
        // The old file is gone: the entry is stale and points here after a rewrite.
        assert!(autostart_is_stale(&old, here, |_| false));
        // Another copy still exists there (an installed AppImage next to a dev build): leave it.
        assert!(!autostart_is_stale(&old, here, |_| true));
        // Already ours.
        let ours = desktop_entry(here, true);
        assert!(!autostart_is_stale(&ours, here, |_| false));
    }

    #[test]
    fn hidden_entry_counts_as_off() {
        let entry = desktop_entry(Path::new("/apps/bloop"), true);
        assert!(autostart_enabled(&entry));
        assert!(!autostart_enabled(&format!(
            "{entry}Hidden=true
"
        )));
    }

    #[test]
    fn xdg_paths() {
        let vars = env(&[("HOME", "/home/u")]);
        assert_eq!(
            autostart_path(&vars),
            Some(PathBuf::from(
                "/home/u/.config/autostart/io.github.LaurentMaxhuni.bloop.desktop"
            ))
        );
        assert_eq!(
            launcher_path(&vars),
            Some(PathBuf::from(
                "/home/u/.local/share/applications/io.github.LaurentMaxhuni.bloop.desktop"
            ))
        );
        let custom = env(&[
            ("HOME", "/home/u"),
            ("XDG_CONFIG_HOME", "/cfg"),
            ("XDG_DATA_HOME", "/data"),
        ]);
        assert_eq!(
            autostart_path(&custom),
            Some(PathBuf::from(
                "/cfg/autostart/io.github.LaurentMaxhuni.bloop.desktop"
            ))
        );
        assert_eq!(
            icon_path(&custom),
            Some(PathBuf::from("/data/icons/hicolor/256x256/apps/bloop.png"))
        );
        assert_eq!(autostart_path(&env(&[])), None);
    }

    #[test]
    fn rules_match_notch_title() {
        for kind in [Tiling::Hyprland, Tiling::Sway, Tiling::I3] {
            assert!(rule(kind).contains("bloop notch"), "{kind:?}");
        }
    }
}
