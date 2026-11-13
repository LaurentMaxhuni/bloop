//! Linux self-update: replace the running AppImage with the downloaded one, then relaunch it.

use std::path::{Path, PathBuf};

/// How this copy takes an update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallMode {
    /// Swap the AppImage file in place.
    Replace { appimage: PathBuf },
    /// Can't replace itself (not an AppImage, or its folder isn't writable): link to the release.
    ReleasePage,
}

/// The temporary file next to the AppImage during the swap.
pub const TEMP_NAME: &str = ".bloop-update.tmp";

pub fn install_mode(appimage: Option<&Path>, dir_writable: bool) -> InstallMode {
    match appimage {
        Some(path) if dir_writable => InstallMode::Replace {
            appimage: path.to_path_buf(),
        },
        _ => InstallMode::ReleasePage,
    }
}

/// Copies `new_file` next to `appimage`, makes it executable and renames it over `appimage`
/// (atomic on one filesystem). On any error the temporary file is removed and the original
/// AppImage is untouched.
#[cfg(unix)]
pub fn replace(new_file: &Path, appimage: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let dir = appimage
        .parent()
        .ok_or_else(|| std::io::Error::other("the AppImage has no folder"))?;
    let temp = dir.join(TEMP_NAME);
    let result = (|| {
        std::fs::copy(new_file, &temp)?;
        std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(0o755))?;
        std::fs::rename(&temp, appimage)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

/// Starts the new AppImage once this copy (`pid`) has exited, so its single-instance check
/// doesn't see the old copy. Waits at most ~10 s. Path and pid are arguments, not script.
pub fn relaunch_command(appimage: &Path, pid: u32) -> std::process::Command {
    let mut command = std::process::Command::new("sh");
    command
        .arg("-c")
        .arg(
            "i=0; while kill -0 \"$1\" 2>/dev/null && [ \"$i\" -lt 100 ]; do sleep 0.1;              i=$((i+1)); done; exec \"$0\" --background",
        )
        .arg(appimage)
        .arg(pid.to_string());
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_mode_without_appimage_opens_release_page() {
        assert_eq!(install_mode(None, true), InstallMode::ReleasePage);
    }

    #[test]
    fn install_mode_unwritable_dir_opens_release_page() {
        assert_eq!(
            install_mode(Some(Path::new("/opt/bloop.AppImage")), false),
            InstallMode::ReleasePage
        );
    }

    #[test]
    fn install_mode_writable_replaces() {
        assert_eq!(
            install_mode(Some(Path::new("/home/u/Apps/bloop.AppImage")), true),
            InstallMode::Replace {
                appimage: PathBuf::from("/home/u/Apps/bloop.AppImage")
            }
        );
    }

    #[test]
    fn relaunch_waits_for_this_copy_then_starts_in_background() {
        let command = relaunch_command(Path::new("/a b/bloop.AppImage"), 4242);
        assert_eq!(command.get_program(), "sh");
        let args: Vec<_> = command.get_args().map(|a| a.to_string_lossy()).collect();
        assert_eq!(args[0], "-c");
        assert!(args[1].contains("kill -0 \"$1\""), "{}", args[1]);
        assert!(args[1].contains("exec \"$0\" --background"), "{}", args[1]);
        // The path and pid are positional arguments, never part of the script.
        assert_eq!(&args[2..], ["/a b/bloop.AppImage", "4242"]);
    }

    #[cfg(unix)]
    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bloop-upd-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[cfg(unix)]
    #[test]
    fn replace_swaps_file_and_keeps_it_executable() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp("swap");
        let (appimage, new) = (dir.join("bloop.AppImage"), dir.join("download"));
        std::fs::write(&appimage, "A").unwrap();
        std::fs::write(&new, "B").unwrap();
        replace(&new, &appimage).unwrap();
        assert_eq!(std::fs::read_to_string(&appimage).unwrap(), "B");
        let mode = std::fs::metadata(&appimage).unwrap().permissions().mode();
        assert_ne!(mode & 0o111, 0);
        assert!(!dir.join(TEMP_NAME).exists());
    }

    #[cfg(unix)]
    #[test]
    fn replace_failure_leaves_original() {
        let dir = temp("fail");
        let appimage = dir.join("bloop.AppImage");
        std::fs::write(&appimage, "A").unwrap();
        assert!(replace(&dir.join("missing"), &appimage).is_err());
        assert_eq!(std::fs::read_to_string(&appimage).unwrap(), "A");
        assert!(!dir.join(TEMP_NAME).exists());
    }
}
