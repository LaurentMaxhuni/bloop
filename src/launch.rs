//! How the app was started (manual or background) and where that routes the user (pure).

/// How this process was launched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Launch {
    /// Started by the user (double-click, Start menu, shortcut).
    Manual,
    /// Started with `--background`, e.g. by login autostart.
    Background,
}

/// Which screen the app opens to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Startup {
    Onboarding,
    Dashboard,
    NotchOnly,
}

/// `Background` if any argument after the program path is `--background`; otherwise `Manual`.
pub fn launch_kind(args: impl IntoIterator<Item = String>) -> Launch {
    if args.into_iter().skip(1).any(|a| a == "--background") {
        Launch::Background
    } else {
        Launch::Manual
    }
}

/// Onboarding until setup is done; then a manual launch opens the dashboard and a
/// background launch stays in the notch.
pub fn startup(launch: Launch, onboarded: bool) -> Startup {
    match (onboarded, launch) {
        (false, _) => Startup::Onboarding,
        (true, Launch::Manual) => Startup::Dashboard,
        (true, Launch::Background) => Startup::NotchOnly,
    }
}

/// Only a manual launch tells an already-running copy to show its window.
pub fn signals_running_copy(launch: Launch) -> bool {
    launch == Launch::Manual
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn no_args_is_manual() {
        assert_eq!(launch_kind(args(&["bloop.exe"])), Launch::Manual);
    }

    #[test]
    fn background_flag() {
        assert_eq!(
            launch_kind(args(&["bloop.exe", "--background"])),
            Launch::Background
        );
    }

    #[test]
    fn unknown_args_ignored() {
        assert_eq!(
            launch_kind(args(&["bloop.exe", "--foo", "x"])),
            Launch::Manual
        );
    }

    #[test]
    fn program_path_named_like_flag_is_not_the_flag() {
        assert_eq!(launch_kind(args(&["--background"])), Launch::Manual);
    }

    #[test]
    fn routing_table() {
        assert_eq!(startup(Launch::Manual, false), Startup::Onboarding);
        assert_eq!(startup(Launch::Background, false), Startup::Onboarding);
        assert_eq!(startup(Launch::Manual, true), Startup::Dashboard);
        assert_eq!(startup(Launch::Background, true), Startup::NotchOnly);
    }

    #[test]
    fn only_manual_launches_signal() {
        assert!(signals_running_copy(Launch::Manual));
        assert!(!signals_running_copy(Launch::Background));
    }
}
