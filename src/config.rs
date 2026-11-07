use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Provider {
    Ollama,
    OpenAi,
    Unsloth,
    ClaudeCode,
    Codex,
}

/// How bloop talks to a provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wire {
    /// Ollama's NDJSON `/api/chat`.
    Ollama,
    /// OpenAI-compatible SSE `/chat/completions`.
    OpenAi,
    /// A local CLI run once per message (chat only).
    Cli,
}

impl Provider {
    pub const ALL: [Provider; 5] = [
        Self::Ollama,
        Self::OpenAi,
        Self::Unsloth,
        Self::ClaudeCode,
        Self::Codex,
    ];

    pub fn wire(self) -> Wire {
        match self {
            Self::Ollama => Wire::Ollama,
            Self::OpenAi | Self::Unsloth => Wire::OpenAi,
            Self::ClaudeCode | Self::Codex => Wire::Cli,
        }
    }

    pub fn is_cli(self) -> bool {
        self.wire() == Wire::Cli
    }

    /// Endpoint filled in when the provider is picked; empty for CLIs.
    pub fn default_endpoint(self) -> &'static str {
        match self {
            Self::Ollama => "http://127.0.0.1:11434",
            Self::OpenAi => "http://127.0.0.1:8080/v1",
            Self::Unsloth => "http://127.0.0.1:8888/v1",
            Self::ClaudeCode | Self::Codex => "",
        }
    }
}

impl std::fmt::Display for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Ollama => "Ollama",
            Self::OpenAi => "OpenAI / llama.cpp",
            Self::Unsloth => "Unsloth Studio",
            Self::ClaudeCode => "Claude Code",
            Self::Codex => "Codex",
        })
    }
}

/// Light, dark, or whatever Windows apps use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ThemeChoice {
    Light,
    #[default]
    Dark,
    System,
}

/// Colour for buttons, focus rings and the mascot's cheeks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Accent {
    #[default]
    Neutral,
    Indigo,
    Teal,
    Rose,
    Amber,
    Lime,
}

impl Accent {
    pub const ALL: [Accent; 6] = [
        Accent::Neutral,
        Accent::Indigo,
        Accent::Teal,
        Accent::Rose,
        Accent::Amber,
        Accent::Lime,
    ];

    /// The accent's sRGB colour, or `None` for the neutral (shadcn) look.
    pub fn rgb(self) -> Option<u32> {
        match self {
            Accent::Neutral => None,
            Accent::Indigo => Some(0x4f46e5),
            Accent::Teal => Some(0x0d9488),
            Accent::Rose => Some(0xe11d48),
            Accent::Amber => Some(0xd97706),
            Accent::Lime => Some(0x65a30d),
        }
    }
}

impl std::fmt::Display for Accent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}

/// How wide the notch's pill and panel are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum NotchWidth {
    S,
    #[default]
    M,
    L,
}

impl NotchWidth {
    pub const ALL: [NotchWidth; 3] = [NotchWidth::S, NotchWidth::M, NotchWidth::L];
    pub const SLIVER: (f32, f32) = (120.0, 5.0);

    /// The pill shown on hover and for peeks.
    pub fn compact(self) -> (f32, f32) {
        match self {
            NotchWidth::S => (260.0, 40.0),
            NotchWidth::M => (300.0, 40.0),
            NotchWidth::L => (360.0, 40.0),
        }
    }

    /// The open chat panel.
    pub fn panel(self) -> (f32, f32) {
        match self {
            NotchWidth::S => (480.0, 580.0),
            NotchWidth::M => (540.0, 580.0),
            NotchWidth::L => (600.0, 580.0),
        }
    }
}

/// Where the notch sits along the top of the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum NotchPosition {
    #[default]
    Centre,
    Left,
    Right,
}

impl NotchPosition {
    pub const ALL: [NotchPosition; 3] = [
        NotchPosition::Centre,
        NotchPosition::Left,
        NotchPosition::Right,
    ];
}

impl std::fmt::Display for NotchPosition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}

/// Whether to skip motion: follow Windows' "Animation effects", or force it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ReduceMotion {
    #[default]
    System,
    On,
    Off,
}

pub const HOVER_DELAYS_MS: [u16; 3] = [100, 150, 300];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, from = "StoredConfig")]
pub struct Config {
    pub provider: Provider,
    pub endpoint: String,
    pub model: String,
    pub shortcut: String,
    pub theme: ThemeChoice,
    /// First-launch setup finished; old configs without it load as false.
    pub onboarded: bool,
    pub accent: Accent,
    pub motion: crate::motion::Feel,
    pub reduce_motion: ReduceMotion,
    pub notch_width: NotchWidth,
    pub notch_position: NotchPosition,
    pub mascot: bool,
    pub hover_delay_ms: u16,
}

/// The saved shape, which may still carry the old `light` flag instead of `theme`.
#[derive(Deserialize, Default)]
#[serde(default)]
struct StoredConfig {
    #[serde(flatten)]
    config: ConfigFields,
    light: Option<bool>,
    theme: Option<ThemeChoice>,
}

/// Every field except the theme, with today's defaults.
#[derive(Deserialize)]
#[serde(default)]
struct ConfigFields {
    provider: Provider,
    endpoint: String,
    model: String,
    shortcut: String,
    onboarded: bool,
    accent: Accent,
    motion: crate::motion::Feel,
    reduce_motion: ReduceMotion,
    notch_width: NotchWidth,
    notch_position: NotchPosition,
    mascot: bool,
    hover_delay_ms: u16,
}

impl Default for ConfigFields {
    fn default() -> Self {
        let c = Config::default();
        Self {
            provider: c.provider,
            endpoint: c.endpoint,
            model: c.model,
            shortcut: c.shortcut,
            onboarded: c.onboarded,
            accent: c.accent,
            motion: c.motion,
            reduce_motion: c.reduce_motion,
            notch_width: c.notch_width,
            notch_position: c.notch_position,
            mascot: c.mascot,
            hover_delay_ms: c.hover_delay_ms,
        }
    }
}

impl From<StoredConfig> for Config {
    fn from(stored: StoredConfig) -> Self {
        let f = stored.config;
        Self {
            provider: f.provider,
            endpoint: f.endpoint,
            model: f.model,
            shortcut: f.shortcut,
            theme: stored.theme.unwrap_or(match stored.light {
                Some(true) => ThemeChoice::Light,
                _ => ThemeChoice::Dark,
            }),
            onboarded: f.onboarded,
            accent: f.accent,
            motion: f.motion,
            reduce_motion: f.reduce_motion,
            notch_width: f.notch_width,
            notch_position: f.notch_position,
            mascot: f.mascot,
            hover_delay_ms: f.hover_delay_ms,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            provider: Provider::Ollama,
            endpoint: "http://127.0.0.1:11434".into(),
            model: "qwen3:4b".into(),
            shortcut: "Ctrl+Alt+B".into(),
            theme: ThemeChoice::Dark,
            onboarded: false,
            accent: Accent::Neutral,
            motion: crate::motion::Feel::Smooth,
            reduce_motion: ReduceMotion::System,
            notch_width: NotchWidth::M,
            notch_position: NotchPosition::Centre,
            mascot: true,
            hover_delay_ms: 150,
        }
    }
}

pub fn validate_endpoint(value: &str) -> Result<reqwest::Url, String> {
    let url = reqwest::Url::parse(value.trim()).map_err(|_| "Enter a valid endpoint URL.")?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("Endpoint must not contain credentials, a query, or a fragment.".into());
    }
    let host = url.host_str().unwrap_or_default().trim_matches(['[', ']']);
    let local = host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback());
    if url.scheme() != "https" && !(url.scheme() == "http" && local) {
        return Err("Use HTTPS for remote providers; HTTP is allowed only on loopback.".into());
    }
    Ok(url)
}

impl Config {
    pub fn validate(&self) -> Result<(), String> {
        if self.provider.is_cli() {
            // The model becomes a CLI argument, so it may never look like a flag or contain spaces.
            let model = self.model.trim();
            if model.len() > 200 {
                return Err("Model must contain at most 200 bytes.".into());
            }
            if model.starts_with('-')
                || !model
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "._:/-".contains(c))
            {
                return Err("Model may contain only letters, digits and . _ : / -".into());
            }
        } else {
            validate_endpoint(&self.endpoint)?;
            if self.model.trim().is_empty() || self.model.len() > 200 {
                return Err("Model must contain 1–200 bytes.".into());
            }
        }
        if !HOVER_DELAYS_MS.contains(&self.hover_delay_ms) {
            return Err("Hover delay must be 100, 150 or 300 ms.".into());
        }
        if self.shortcut.trim().is_empty() || self.shortcut.len() > 100 {
            return Err("Enter a global shortcut, such as Ctrl+Alt+B.".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn plaintext_remote_endpoint_is_rejected() {
        assert!(super::validate_endpoint("http://example.com:11434").is_err());
        assert!(super::validate_endpoint("http://127.0.0.1:11434").is_ok());
        assert!(super::validate_endpoint("https://example.com/v1").is_ok());
        assert!(super::validate_endpoint("https://user:secret@example.com").is_err());
    }

    #[test]
    fn old_config_without_onboarded_loads_as_false() {
        let old: super::Config = serde_json::from_str(
            r#"{"provider":"Ollama","endpoint":"http://127.0.0.1:11434","model":"m","shortcut":"Ctrl+Alt+B","light":true}"#,
        )
        .unwrap();
        assert!(!old.onboarded);
        assert_eq!(old.theme, super::ThemeChoice::Light);
        let done = super::Config {
            onboarded: true,
            ..Default::default()
        };
        let back: super::Config =
            serde_json::from_str(&serde_json::to_string(&done).unwrap()).unwrap();
        assert!(back.onboarded);
    }

    #[test]
    fn old_light_true_loads_as_light_theme() {
        let c: super::Config = serde_json::from_str(r#"{"light":true}"#).unwrap();
        assert_eq!(c.theme, super::ThemeChoice::Light);
    }

    #[test]
    fn old_light_false_loads_as_dark() {
        let c: super::Config = serde_json::from_str(r#"{"light":false}"#).unwrap();
        assert_eq!(c.theme, super::ThemeChoice::Dark);
        let none: super::Config = serde_json::from_str("{}").unwrap();
        assert_eq!(none.theme, super::ThemeChoice::Dark);
    }

    #[test]
    fn new_theme_wins_over_old_light_and_light_is_not_written() {
        let c: super::Config = serde_json::from_str(r#"{"light":true,"theme":"System"}"#).unwrap();
        assert_eq!(c.theme, super::ThemeChoice::System);
        let json = serde_json::to_string(&c).unwrap();
        assert!(!json.contains("\"light\""), "{json}");
    }

    #[test]
    fn defaults_match_today() {
        use super::*;
        let c = Config::default();
        assert_eq!(c.notch_width.compact(), (300.0, 40.0));
        assert_eq!(c.notch_width.panel(), (540.0, 580.0));
        assert_eq!(NotchWidth::S.compact(), (260.0, 40.0));
        assert_eq!(NotchWidth::L.panel(), (600.0, 580.0));
        assert_eq!(c.notch_position, NotchPosition::Centre);
        assert_eq!(c.motion, crate::motion::Feel::Smooth);
        assert_eq!(c.reduce_motion, ReduceMotion::System);
        assert_eq!(c.accent, Accent::Neutral);
        assert_eq!(c.theme, ThemeChoice::Dark);
        assert!(c.mascot);
        assert_eq!(c.hover_delay_ms, 150);
    }

    #[test]
    fn new_fields_round_trip() {
        use super::*;
        let c = Config {
            theme: ThemeChoice::System,
            accent: Accent::Teal,
            motion: crate::motion::Feel::Calm,
            reduce_motion: ReduceMotion::On,
            notch_width: NotchWidth::L,
            notch_position: NotchPosition::Right,
            mascot: false,
            hover_delay_ms: 300,
            ..Config::default()
        };
        let back: Config = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        assert_eq!(back.accent, Accent::Teal);
        assert_eq!(back.notch_position, NotchPosition::Right);
        assert_eq!(back.theme, ThemeChoice::System);
        assert!(!back.mascot);
        assert_eq!(back.hover_delay_ms, 300);
    }

    #[test]
    fn bad_hover_delay_rejected() {
        let c = super::Config {
            hover_delay_ms: 42,
            ..super::Config::default()
        };
        assert!(c.validate().is_err());
    }

    #[test]
    fn old_provider_names_still_load() {
        let config: super::Config = serde_json::from_str(r#"{"provider":"OpenAi"}"#).unwrap();
        assert_eq!(config.provider, super::Provider::OpenAi);
    }

    #[test]
    fn cli_model_may_be_blank_but_not_flags() {
        use super::{Config, Provider};
        let cli = |model: &str| Config {
            provider: Provider::ClaudeCode,
            endpoint: String::new(),
            model: model.into(),
            ..Config::default()
        };
        assert!(cli("").validate().is_ok());
        assert!(cli("claude-sonnet-4.5").validate().is_ok());
        assert!(cli("--dangerous").validate().is_err());
        assert!(cli("a b").validate().is_err());
        let ollama = Config {
            model: String::new(),
            ..Config::default()
        };
        assert!(ollama.validate().is_err());
    }

    #[test]
    fn unsloth_uses_openai_wire() {
        use super::{Provider, Wire};
        assert_eq!(Provider::Unsloth.wire(), Wire::OpenAi);
        assert_eq!(Provider::Codex.wire(), Wire::Cli);
        assert!(Provider::ClaudeCode.is_cli());
        assert!(!Provider::Unsloth.is_cli());
        assert_eq!(
            Provider::Unsloth.default_endpoint(),
            "http://127.0.0.1:8888/v1"
        );
        assert_eq!(Provider::ALL.len(), 5);
    }
}
