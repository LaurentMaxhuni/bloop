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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub provider: Provider,
    pub endpoint: String,
    pub model: String,
    pub shortcut: String,
    pub light: bool,
    /// First-launch setup finished; old configs without it load as false.
    pub onboarded: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            provider: Provider::Ollama,
            endpoint: "http://127.0.0.1:11434".into(),
            model: "qwen3:4b".into(),
            shortcut: "Ctrl+Alt+B".into(),
            light: false,
            onboarded: false,
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
        assert!(old.light);
        let done = super::Config {
            onboarded: true,
            ..Default::default()
        };
        let back: super::Config =
            serde_json::from_str(&serde_json::to_string(&done).unwrap()).unwrap();
        assert!(back.onboarded);
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
        assert_eq!(Provider::Unsloth.default_endpoint(), "http://127.0.0.1:8888/v1");
        assert_eq!(Provider::ALL.len(), 5);
    }
}
