use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Provider {
    Ollama,
    OpenAi,
    Unsloth,
    ClaudeCode,
    Codex,
    ChatGpt,
}

/// How bloop talks to a provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wire {
    /// Ollama's NDJSON `/api/chat`.
    Ollama,
    /// OpenAI-compatible SSE `/chat/completions`.
    OpenAi,
    Responses,
    /// A local CLI run once per message (chat only).
    Cli,
}

impl Provider {
    pub const ALL: [Provider; 6] = [
        Self::Ollama,
        Self::OpenAi,
        Self::Unsloth,
        Self::ClaudeCode,
        Self::Codex,
        Self::ChatGpt,
    ];

    pub fn wire(self) -> Wire {
        match self {
            Self::Ollama => Wire::Ollama,
            Self::OpenAi | Self::Unsloth => Wire::OpenAi,
            Self::ChatGpt => Wire::Responses,
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
            Self::ClaudeCode | Self::Codex | Self::ChatGpt => "",
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
            Self::ChatGpt => "ChatGPT",
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

/// How much completed chat history accompanies the next prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ContextMode {
    #[default]
    Full,
    Last3,
    OnlyMessage,
}

impl ContextMode {
    pub const ALL: [Self; 3] = [Self::Full, Self::Last3, Self::OnlyMessage];

    pub fn max_turns(self) -> usize {
        match self {
            Self::Full => 12,
            Self::Last3 => 3,
            Self::OnlyMessage => 0,
        }
    }
}

impl std::fmt::Display for ContextMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Full => "Full chat",
            Self::Last3 => "Last 3 turns",
            Self::OnlyMessage => "Only this message",
        })
    }
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
    /// Global reply preferences, sent with every message across all profiles.
    pub instructions: String,
    pub context: ContextMode,
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
    /// The mascot's hat, glasses, outfit and colour.
    pub look: Look,
    /// Look for new versions on GitHub every six hours.
    pub auto_update: bool,
    pub hover_delay_ms: u16,
    /// Global shortcut that opens the notch with the clipboard's text attached; empty = off.
    pub ask_shortcut: String,
    /// Saved model profiles besides the main one (provider, endpoint and model above).
    pub profiles: Vec<Profile>,
    /// The profile new chats start on: 0 is the main one, else a saved profile's id.
    pub default_profile: u32,
    /// The highest profile id ever handed out. Ids are never reused, so a chat that kept a
    /// deleted profile's id can't land on a newer, unrelated profile.
    pub last_profile_id: u32,
}

/// A named provider, endpoint and model to switch to from the notch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    /// Never 0: that id means the main profile.
    pub id: u32,
    pub name: String,
    pub provider: Provider,
    pub endpoint: String,
    pub model: String,
}

pub const MAX_PROFILES: usize = 8;
pub const MAX_INSTRUCTIONS: usize = 2048;

/// The name shown for profile 0, the provider, endpoint and model kept at the top level.
pub const MAIN_PROFILE: &str = "Main";

/// Where a chat's messages go. Context collected for one destination is never sent to
/// another, so a chat keeps the destination of its first message.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Destination {
    /// A model server on this computer (loopback endpoint).
    Local,
    /// Anything else, named by provider or endpoint origin.
    Cloud(String),
}

impl Destination {
    /// Stable text form saved with a chat: "local" or "cloud:<name>".
    pub fn key(&self) -> String {
        match self {
            Destination::Local => "local".into(),
            Destination::Cloud(name) => format!("cloud:{name}"),
        }
    }

    /// Reads `key()`'s form back; anything unknown counts as cloud.
    pub fn from_key(key: &str) -> Destination {
        match key.strip_prefix("cloud:") {
            _ if key == "local" => Destination::Local,
            Some(name) => Destination::Cloud(name.into()),
            None => Destination::Cloud(key.into()),
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Destination::Local => "Local",
            Destination::Cloud(_) => "Cloud",
        }
    }

    /// Who receives the chat, for messages: "this computer", "ChatGPT", a host name.
    pub fn describe(&self) -> String {
        match self {
            Destination::Local => "this computer".into(),
            Destination::Cloud(name) => name
                .split_once("://")
                .map_or(name.as_str(), |(_, host)| host)
                .to_owned(),
        }
    }
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
    instructions: String,
    context: ContextMode,
    shortcut: String,
    onboarded: bool,
    accent: Accent,
    motion: crate::motion::Feel,
    reduce_motion: ReduceMotion,
    notch_width: NotchWidth,
    notch_position: NotchPosition,
    mascot: bool,
    look: Look,
    hover_delay_ms: u16,
    auto_update: bool,
    ask_shortcut: String,
    profiles: Vec<Profile>,
    default_profile: u32,
    last_profile_id: u32,
}

impl Default for ConfigFields {
    fn default() -> Self {
        let c = Config::default();
        Self {
            provider: c.provider,
            endpoint: c.endpoint,
            model: c.model,
            instructions: c.instructions,
            context: c.context,
            shortcut: c.shortcut,
            onboarded: c.onboarded,
            accent: c.accent,
            motion: c.motion,
            reduce_motion: c.reduce_motion,
            notch_width: c.notch_width,
            notch_position: c.notch_position,
            mascot: c.mascot,
            look: c.look,
            auto_update: c.auto_update,
            hover_delay_ms: c.hover_delay_ms,
            ask_shortcut: c.ask_shortcut,
            profiles: c.profiles,
            default_profile: c.default_profile,
            last_profile_id: c.last_profile_id,
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
            instructions: f.instructions.trim().to_owned(),
            context: f.context,
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
            look: f.look,
            auto_update: f.auto_update,
            // Hand-edited or future values snap to the nearest choice instead of failing to load.
            hover_delay_ms: *HOVER_DELAYS_MS
                .iter()
                .min_by_key(|&&ms| ms.abs_diff(f.hover_delay_ms))
                .unwrap_or(&150),
            ask_shortcut: f.ask_shortcut,
            // A hand-edited default that names no saved profile falls back to the main one.
            default_profile: if f.profiles.iter().any(|p| p.id == f.default_profile) {
                f.default_profile
            } else {
                0
            },
            // Never below a saved profile's id, even if the counter was edited by hand.
            last_profile_id: f
                .profiles
                .iter()
                .map(|p| p.id)
                .fold(f.last_profile_id, u32::max),
            profiles: f.profiles,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            provider: Provider::Ollama,
            endpoint: "http://127.0.0.1:11434".into(),
            model: "qwen3:4b".into(),
            instructions: String::new(),
            context: ContextMode::Full,
            shortcut: "Ctrl+Alt+B".into(),
            theme: ThemeChoice::Dark,
            onboarded: false,
            accent: Accent::Neutral,
            motion: crate::motion::Feel::Smooth,
            reduce_motion: ReduceMotion::System,
            notch_width: NotchWidth::M,
            notch_position: NotchPosition::Centre,
            mascot: true,
            look: Look::default(),
            auto_update: true,
            hover_delay_ms: 150,
            ask_shortcut: "Ctrl+Alt+Shift+A".into(),
            profiles: Vec::new(),
            default_profile: 0,
            last_profile_id: 0,
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

/// Checks one provider, endpoint and model combination.
pub fn validate_model(provider: Provider, endpoint: &str, model: &str) -> Result<(), String> {
    if provider.is_cli() {
        // The model becomes a CLI argument, so it may never look like a flag or contain spaces.
        let model = model.trim();
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
    } else if provider == Provider::ChatGpt {
        if model.len() > 200 {
            return Err("Model must contain at most 200 bytes.".into());
        }
    } else {
        validate_endpoint(endpoint)?;
        if model.trim().is_empty() || model.len() > 200 {
            return Err("Model must contain 1–200 bytes.".into());
        }
    }
    Ok(())
}

/// Where messages for `provider` at `endpoint` end up.
pub fn destination(provider: Provider, endpoint: &str) -> Destination {
    match provider {
        Provider::ClaudeCode => Destination::Cloud("Claude Code".into()),
        Provider::Codex => Destination::Cloud("Codex".into()),
        Provider::ChatGpt => Destination::Cloud("ChatGPT".into()),
        Provider::Ollama | Provider::OpenAi | Provider::Unsloth => {
            match validate_endpoint(endpoint) {
                Ok(url) if url.scheme() == "http" => Destination::Local,
                Ok(url) => Destination::Cloud(url.origin().ascii_serialization()),
                // Invalid endpoints never send; name them so they never match a real one.
                Err(_) => Destination::Cloud(format!("invalid:{endpoint}")),
            }
        }
    }
}

impl Profile {
    pub fn validate(&self) -> Result<(), String> {
        let name = self.name.trim();
        if name.is_empty() || name.chars().count() > 40 {
            return Err("Give the profile a name of 1–40 characters.".into());
        }
        if self.id == 0 {
            return Err("Profile id 0 is reserved for the main profile.".into());
        }
        validate_model(self.provider, &self.endpoint, &self.model)
    }
}

impl Config {
    pub fn validate(&self) -> Result<(), String> {
        validate_model(self.provider, &self.endpoint, &self.model)?;
        if self.instructions.trim().len() > MAX_INSTRUCTIONS {
            return Err(format!(
                "Custom instructions must contain at most {MAX_INSTRUCTIONS} bytes."
            ));
        }
        if self.shortcut.trim().is_empty() || self.shortcut.len() > 100 {
            return Err("Enter a global shortcut, such as Ctrl+Alt+B.".into());
        }
        if self.ask_shortcut.len() > 100 {
            return Err("The clipboard shortcut is too long.".into());
        }
        if self.profiles.len() > MAX_PROFILES {
            return Err(format!("Keep at most {MAX_PROFILES} saved profiles."));
        }
        let mut ids = std::collections::HashSet::new();
        for profile in &self.profiles {
            profile.validate()?;
            if !ids.insert(profile.id) {
                return Err("Two profiles share an id.".into());
            }
        }
        if self.default_profile != 0 && !ids.contains(&self.default_profile) {
            return Err("The default profile no longer exists.".into());
        }
        Ok(())
    }

    /// Where this config's default model sends messages.
    pub fn destination(&self) -> Destination {
        destination(self.provider, &self.endpoint)
    }

    /// This config with profile `id`'s provider, endpoint and model swapped in; 0 is the
    /// main profile. `None` when the profile no longer exists.
    pub fn with_profile(&self, id: u32) -> Option<Config> {
        if id == 0 {
            return Some(self.clone());
        }
        let profile = self.profiles.iter().find(|p| p.id == id)?;
        Some(Config {
            provider: profile.provider,
            endpoint: profile.endpoint.clone(),
            model: profile.model.clone(),
            ..self.clone()
        })
    }

    /// The display name of profile `id`.
    pub fn profile_name(&self, id: u32) -> Option<&str> {
        if id == 0 {
            return Some(MAIN_PROFILE);
        }
        self.profiles
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.name.as_str())
    }

    /// The profile new chats start on, falling back to the main one.
    pub fn default_profile(&self) -> u32 {
        if self.profile_name(self.default_profile).is_some() {
            self.default_profile
        } else {
            0
        }
    }

    /// An id no profile has ever used, saved or deleted.
    pub fn next_profile_id(&self) -> u32 {
        self.profiles
            .iter()
            .map(|p| p.id)
            .max()
            .unwrap_or(0)
            .max(self.last_profile_id)
            + 1
    }
}

macro_rules! choice {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $label:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
        pub enum $name {
            #[default]
            $($variant),+
        }

        impl $name {
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            pub fn label(self) -> &'static str {
                match self {
                    $($name::$variant => $label),+
                }
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.label())
            }
        }
    };
}

choice!(
    /// Something on Bloop's head; any hat hides the antenna.
    Hat {
        None => "None",
        TopHat => "Top hat",
        Beanie => "Beanie",
        Cap => "Cap",
        Crown => "Crown",
        Party => "Party",
        Wizard => "Wizard",
    }
);

choice!(
    /// Something over Bloop's eyes.
    Eyewear {
        None => "None",
        Sunglasses => "Shades",
        Round => "Glasses",
        Monocle => "Monocle",
    }
);

choice!(
    /// Something Bloop wears below the face.
    Outfit {
        None => "None",
        BowTie => "Bow tie",
        Scarf => "Scarf",
        Robe => "Robe",
        Cape => "Cape",
    }
);

choice!(
    /// Bloop's body colour; Classic follows the theme's text colour.
    Coat {
        Classic => "Classic",
        Mint => "Mint",
        Peach => "Peach",
        Lilac => "Lilac",
        Sky => "Sky",
        Lemon => "Lemon",
    }
);

/// How Bloop is dressed. Saved with the settings; anything unknown loads as the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Look {
    pub hat: Hat,
    pub eyewear: Eyewear,
    pub outfit: Outfit,
    pub coat: Coat,
}

impl Look {
    /// A random outfit that differs from `self`, seeded by the caller (no extra dependency).
    pub fn shuffled(self, seed: u64) -> Look {
        let pick = |n: usize, shift: u32| ((seed >> shift) as usize) % n;
        let next = Look {
            hat: Hat::ALL[pick(Hat::ALL.len(), 0)],
            eyewear: Eyewear::ALL[pick(Eyewear::ALL.len(), 8)],
            outfit: Outfit::ALL[pick(Outfit::ALL.len(), 16)],
            coat: Coat::ALL[pick(Coat::ALL.len(), 24)],
        };
        if next == self {
            Look {
                hat: Hat::ALL[(pick(Hat::ALL.len(), 0) + 1) % Hat::ALL.len()],
                ..next
            }
        } else {
            next
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn context_defaults_and_round_trips() {
        use super::{Config, ContextMode};

        let old: Config = serde_json::from_str("{}").unwrap();
        assert_eq!(old.context, ContextMode::Full);
        for mode in ContextMode::ALL {
            let config = Config {
                context: mode,
                ..Config::default()
            };
            let back: Config =
                serde_json::from_str(&serde_json::to_string(&config).unwrap()).unwrap();
            assert_eq!(back.context, mode);
        }
    }

    #[test]
    fn mascot_look_round_trips_and_unknown_parts_fall_back() {
        use super::{Coat, Eyewear, Hat, Look, Outfit};
        let old: super::Config = serde_json::from_str("{}").unwrap();
        assert_eq!(old.look, Look::default());
        let look = Look {
            hat: Hat::Wizard,
            eyewear: Eyewear::Sunglasses,
            outfit: Outfit::Robe,
            coat: Coat::Lilac,
        };
        let config = super::Config {
            look,
            ..super::Config::default()
        };
        let back: super::Config =
            serde_json::from_str(&serde_json::to_string(&config).unwrap()).unwrap();
        assert_eq!(back.look, look);
        let partial: super::Config = serde_json::from_str(r#"{"look": {"hat": "Crown"}}"#).unwrap();
        assert_eq!(partial.look.hat, Hat::Crown);
        assert_eq!(partial.look.outfit, Outfit::None);
    }

    #[test]
    fn shuffled_look_always_changes() {
        use super::Look;
        let mut look = Look::default();
        for seed in 0..500u64 {
            let next = look.shuffled(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
            assert_ne!(next, look);
            look = next;
        }
    }

    #[test]
    fn custom_instructions_default_and_round_trip() {
        let old: super::Config = serde_json::from_str("{}").unwrap();
        assert_eq!(serde_json::to_value(old).unwrap()["instructions"], "");
        let config: super::Config = serde_json::from_value(serde_json::json!({
            "instructions": "  I write Rust.\nKeep replies short.  "
        }))
        .unwrap();
        let back: super::Config =
            serde_json::from_str(&serde_json::to_string(&config).unwrap()).unwrap();
        assert_eq!(
            serde_json::to_value(back).unwrap()["instructions"],
            "I write Rust.\nKeep replies short."
        );
    }

    #[test]
    fn custom_instructions_limit_counts_trimmed_utf8_bytes() {
        let config = |instructions: String| -> super::Config {
            serde_json::from_value(serde_json::json!({"instructions": instructions})).unwrap()
        };
        assert!(
            config(format!("  {}\n", "é".repeat(1024)))
                .validate()
                .is_ok()
        );
        assert!(
            super::Config {
                instructions: format!("  {}\n", "é".repeat(1024)),
                ..super::Config::default()
            }
            .validate()
            .is_ok()
        );
        let error = config(format!("{}a", "é".repeat(1024)))
            .validate()
            .unwrap_err();
        assert_eq!(
            error,
            "Custom instructions must contain at most 2048 bytes."
        );
    }

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
    fn config_auto_update_defaults_on() {
        assert!(super::Config::default().auto_update);
        let old: super::Config = serde_json::from_str("{}").unwrap();
        assert!(old.auto_update);
        let off: super::Config = serde_json::from_str(r#"{"auto_update":false}"#).unwrap();
        assert!(!off.auto_update);
    }

    #[test]
    fn odd_hover_delay_snaps_to_nearest_choice() {
        for (saved, loaded) in [(200, 150), (42, 100), (1000, 300), (125, 100), (126, 150)] {
            let c: super::Config =
                serde_json::from_str(&format!(r#"{{"hover_delay_ms":{saved}}}"#)).unwrap();
            assert_eq!(c.hover_delay_ms, loaded, "saved {saved}");
            assert!(c.validate().is_ok());
        }
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
        assert_eq!(Provider::ALL.len(), 6);
    }

    #[test]
    fn destinations_split_local_from_cloud() {
        use super::{Destination, Provider, destination};
        assert_eq!(
            destination(Provider::Ollama, "http://127.0.0.1:11434"),
            Destination::Local
        );
        assert_eq!(
            destination(Provider::OpenAi, "https://api.example.com/v1"),
            Destination::Cloud("https://api.example.com".into())
        );
        assert_eq!(destination(Provider::Codex, "").key(), "cloud:Codex");
        assert_ne!(
            destination(Provider::OpenAi, "https://a.example.com/v1"),
            destination(Provider::OpenAi, "https://b.example.com/v1")
        );
    }

    #[test]
    fn profiles_validate_and_swap_in() {
        use super::{Config, Profile, Provider};
        let mut config = Config::default();
        let work = Profile {
            id: config.next_profile_id(),
            name: "Work".into(),
            provider: Provider::ChatGpt,
            endpoint: String::new(),
            model: String::new(),
        };
        config.profiles.push(work.clone());
        assert!(config.validate().is_ok());
        let swapped = config.with_profile(work.id).unwrap();
        assert_eq!(swapped.provider, Provider::ChatGpt);
        assert_eq!(swapped.shortcut, config.shortcut);
        assert!(config.with_profile(99).is_none());
        assert_eq!(config.profile_name(0), Some("Main"));

        config.profiles.push(work);
        assert!(config.validate().is_err(), "duplicate ids");
        config.profiles.pop();
        config.profiles[0].name = " ".into();
        assert!(config.validate().is_err(), "blank name");
    }

    #[test]
    fn old_config_has_no_profiles_and_the_default_ask_shortcut() {
        let c: super::Config = serde_json::from_str("{}").unwrap();
        assert!(c.profiles.is_empty());
        assert_eq!(c.ask_shortcut, "Ctrl+Alt+Shift+A");
        assert_eq!(c.default_profile, 0);
    }

    #[test]
    fn default_profile_must_name_a_saved_profile() {
        use super::{Config, Profile, Provider};
        let mut config = Config::default();
        config.profiles.push(Profile {
            id: 3,
            name: "Work".into(),
            provider: Provider::Codex,
            endpoint: String::new(),
            model: String::new(),
        });
        config.default_profile = 3;
        assert!(config.validate().is_ok());
        assert_eq!(config.default_profile(), 3);
        config.default_profile = 4;
        assert!(config.validate().is_err());
        assert_eq!(
            config.default_profile(),
            0,
            "falls back to the main profile"
        );

        // A saved file naming a missing profile loads with the main one as default.
        let json = serde_json::json!({ "default_profile": 9 }).to_string();
        let loaded: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.default_profile, 0);
        let json = serde_json::to_string(&config.clone()).unwrap();
        let mut back: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(back.default_profile, 0, "4 isn't saved");
        back.default_profile = 3;
        let json = serde_json::to_string(&back).unwrap();
        let back: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(back.default_profile, 3);
    }
}
