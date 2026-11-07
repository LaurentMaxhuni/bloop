use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Provider {
    Ollama,
    OpenAi,
}

impl std::fmt::Display for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Ollama => "Ollama",
            Self::OpenAi => "OpenAI / llama.cpp",
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
        validate_endpoint(&self.endpoint)?;
        if self.model.trim().is_empty() || self.model.len() > 200 {
            return Err("Model must contain 1–200 bytes.".into());
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
}
