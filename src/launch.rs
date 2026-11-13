//! How the app was started (manual or background) and where that routes the user (pure).

/// How this process was launched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Launch {
    /// Started by the user (double-click, Start menu, shortcut).
    Manual,
    /// Started with `--background`, e.g. by login autostart.
    Background,
    /// Started with `--toggle`: show or collapse the running copy's notch.
    Toggle,
    /// Started with `--ask`: ask the running copy about the clipboard.
    Ask,
}

/// What a later launch asks the running copy to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    Open,
    Toggle,
    Ask,
}

impl Request {
    /// The one-line wire form sent to the running copy.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Toggle => "toggle",
            Self::Ask => "ask",
        }
    }

    /// Reads a wire line; anything else is ignored.
    pub fn parse(line: &str) -> Option<Self> {
        match line.trim() {
            "open" => Some(Self::Open),
            "toggle" => Some(Self::Toggle),
            "ask" => Some(Self::Ask),
            _ => None,
        }
    }
}

/// Which screen the app opens to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Startup {
    Onboarding,
    Dashboard,
    NotchOnly,
}

/// `Background` if any argument after the program path is `--background`; else `Toggle` or
/// `Ask` for `--toggle` / `--ask`; otherwise `Manual`.
pub fn launch_kind(args: impl IntoIterator<Item = String>) -> Launch {
    let args: Vec<String> = args.into_iter().skip(1).collect();
    let has = |flag: &str| args.iter().any(|a| a == flag);
    if has("--background") {
        Launch::Background
    } else if has("--toggle") {
        Launch::Toggle
    } else if has("--ask") {
        Launch::Ask
    } else {
        Launch::Manual
    }
}

/// Onboarding until setup is done; then a manual launch opens the dashboard and a
/// background launch stays in the notch.
pub fn startup(launch: Launch, onboarded: bool) -> Startup {
    match (onboarded, launch) {
        (false, _) => Startup::Onboarding,
        (true, Launch::Manual | Launch::Toggle | Launch::Ask) => Startup::Dashboard,
        (true, Launch::Background) => Startup::NotchOnly,
    }
}

/// What this launch asks an already-running copy to do; a background launch asks nothing.
pub fn request(launch: Launch) -> Option<Request> {
    match launch {
        Launch::Manual => Some(Request::Open),
        Launch::Background => None,
        Launch::Toggle => Some(Request::Toggle),
        Launch::Ask => Some(Request::Ask),
    }
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
    fn background_launches_ask_nothing() {
        assert_eq!(request(Launch::Manual), Some(Request::Open));
        assert_eq!(request(Launch::Background), None);
    }

    #[test]
    fn toggle_and_ask_flags() {
        assert_eq!(launch_kind(args(&["bloop", "--toggle"])), Launch::Toggle);
        assert_eq!(launch_kind(args(&["bloop", "--ask"])), Launch::Ask);
        assert_eq!(
            launch_kind(args(&["bloop", "--toggle", "--background"])),
            Launch::Background
        );
    }

    #[test]
    fn requests_per_launch() {
        assert_eq!(request(Launch::Manual), Some(Request::Open));
        assert_eq!(request(Launch::Background), None);
        assert_eq!(request(Launch::Toggle), Some(Request::Toggle));
        assert_eq!(request(Launch::Ask), Some(Request::Ask));
        assert_eq!(startup(Launch::Ask, true), Startup::Dashboard);
        assert_eq!(startup(Launch::Toggle, false), Startup::Onboarding);
    }
}
