//! Desktop integration files and tiling-desktop window rules.

use std::ffi::OsString;

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
    fn rules_match_notch_title() {
        for kind in [Tiling::Hyprland, Tiling::Sway, Tiling::I3] {
            assert!(rule(kind).contains("bloop notch"), "{kind:?}");
        }
    }
}
