//! First-launch setup: step state (pure) and finding which providers this computer has.

use crate::config::{Config, Provider};

/// One screen of the setup, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Welcome,
    Model,
    HowItWorks,
    MakeItYours,
    AllSet,
}

const STEPS: [Step; Onboarding::COUNT] = [
    Step::Welcome,
    Step::Model,
    Step::HowItWorks,
    Step::MakeItYours,
    Step::AllSet,
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Onboarding {
    index: usize,
}

impl Default for Onboarding {
    fn default() -> Self {
        Self::new()
    }
}

impl Onboarding {
    pub const COUNT: usize = 5;

    pub fn new() -> Self {
        Self { index: 0 }
    }

    pub fn step(&self) -> Step {
        STEPS[self.index]
    }

    pub fn index(&self) -> usize {
        self.index
    }

    pub fn next(&mut self) {
        self.index = (self.index + 1).min(Self::COUNT - 1);
    }

    pub fn back(&mut self) {
        self.index = self.index.saturating_sub(1);
    }

    pub fn is_first(&self) -> bool {
        self.index == 0
    }

    pub fn is_last(&self) -> bool {
        self.index == Self::COUNT - 1
    }
}

/// Result of looking for providers during setup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Detection {
    Checking,
    Found(Vec<Choice>),
    NotFound,
}

/// One ready-to-use provider found on this computer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub provider: Provider,
    pub endpoint: String,
    pub model: String,
    pub label: String,
}

impl std::fmt::Display for Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

impl Choice {
    pub fn ollama(endpoint: &str, model: &str) -> Self {
        Self {
            provider: Provider::Ollama,
            endpoint: endpoint.into(),
            model: model.into(),
            label: format!("Ollama — {model}"),
        }
    }

    pub fn unsloth(endpoint: &str, model: &str) -> Self {
        Self {
            provider: Provider::Unsloth,
            endpoint: endpoint.into(),
            model: model.into(),
            label: format!("Unsloth Studio — {model}"),
        }
    }

    /// Unsloth answered but wants a key before it lists models.
    pub fn unsloth_needs_key(endpoint: &str) -> Self {
        Self {
            provider: Provider::Unsloth,
            endpoint: endpoint.into(),
            model: String::new(),
            label: "Unsloth Studio — paste its key, then Retry".into(),
        }
    }

    pub fn cli(provider: Provider) -> Self {
        Self {
            provider,
            endpoint: String::new(),
            model: String::new(),
            label: provider.to_string(),
        }
    }
}

/// Which found choice to select without asking, if any. Re-running setup must never
/// replace a configuration that works but that detection can't see (a remote endpoint,
/// llama.cpp, another port), so only these count:
/// - the exact provider, endpoint and model;
/// - the same CLI (the caller keeps the user's model);
/// - the same server whose current model isn't installed there (its first model);
/// - untouched first-launch defaults (the first thing found).
pub fn preselect(choices: &[Choice], current: &Config) -> Option<usize> {
    let same_server = |c: &Choice| c.provider == current.provider && c.endpoint == current.endpoint;
    choices
        .iter()
        .position(|c| same_server(c) && c.model == current.model)
        .or_else(|| {
            choices
                .iter()
                .position(|c| c.provider.is_cli() && c.provider == current.provider)
        })
        .or_else(|| choices.iter().position(same_server))
        .or_else(|| {
            let fresh = Config::default();
            (current.provider == fresh.provider
                && current.endpoint == fresh.endpoint
                && current.model == fresh.model
                && !choices.is_empty())
            .then_some(0)
        })
}

/// Looks for Ollama, Unsloth Studio (ports 8888 and 8000) and the Claude Code and Codex
/// CLIs at once. Each HTTP probe gives up after 3 seconds.
pub async fn detect() -> Vec<Choice> {
    use crate::provider::list_models;
    let probe = |provider: Provider, endpoint: &'static str| {
        let config = Config {
            provider,
            endpoint: endpoint.into(),
            ..Config::default()
        };
        async move { (endpoint, list_models(&config).await) }
    };
    let (ollama, unsloth_a, unsloth_b) = tokio::join!(
        probe(Provider::Ollama, "http://127.0.0.1:11434"),
        probe(Provider::Unsloth, "http://127.0.0.1:8888/v1"),
        probe(Provider::Unsloth, "http://127.0.0.1:8000/v1"),
    );
    let mut choices = Vec::new();
    if let (endpoint, Ok(models)) = ollama {
        choices.extend(models.iter().map(|m| Choice::ollama(endpoint, m)));
    }
    for (endpoint, result) in [unsloth_a, unsloth_b] {
        match result {
            Ok(models) if !models.is_empty() => {
                choices.extend(models.iter().map(|m| Choice::unsloth(endpoint, m)));
                break;
            }
            Err(e) if e.to_string().contains("rejected the key") => {
                choices.push(Choice::unsloth_needs_key(endpoint));
                break;
            }
            _ => {}
        }
    }
    for (kind, provider) in [
        (crate::cli::Kind::Claude, Provider::ClaudeCode),
        (crate::cli::Kind::Codex, Provider::Codex),
    ] {
        if crate::cli::find(kind).is_some() {
            choices.push(Choice::cli(provider));
        }
    }
    choices
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_saturates() {
        let mut o = Onboarding::new();
        assert_eq!(o.step(), Step::Welcome);
        assert!(o.is_first());
        o.back();
        assert_eq!(o.step(), Step::Welcome);
        for _ in 0..4 {
            o.next();
        }
        assert_eq!(o.step(), Step::AllSet);
        assert!(o.is_last());
        assert_eq!(o.index(), 4);
        o.next();
        assert_eq!(o.step(), Step::AllSet);
        o.back();
        assert_eq!(o.step(), Step::MakeItYours);
    }

    #[test]
    fn preselect_keeps_current_provider_and_model() {
        use crate::config::{Config, Provider};
        let ollama = "http://127.0.0.1:11434";
        let choices = vec![
            Choice::ollama(ollama, "a"),
            Choice::ollama(ollama, "qwen3:4b"),
            Choice::cli(Provider::Codex),
        ];
        // Exact match.
        assert_eq!(preselect(&choices, &Config::default()), Some(1));
        // Same server, model not installed there: its first model.
        let missing = Config {
            model: "gone".into(),
            ..Config::default()
        };
        assert_eq!(preselect(&choices, &missing), Some(0));
        // Same CLI.
        let codex = Config {
            provider: Provider::Codex,
            model: "gpt-5.5".into(),
            ..Config::default()
        };
        assert_eq!(preselect(&choices, &codex), Some(2));
        // A setup detection never sees (remote endpoint, llama.cpp, other port): untouched.
        let remote = Config {
            provider: Provider::OpenAi,
            endpoint: "https://api.example.com/v1".into(),
            model: "m".into(),
            ..Config::default()
        };
        assert_eq!(preselect(&choices, &remote), None);
        let other_port = Config {
            endpoint: "http://127.0.0.1:11500".into(),
            ..Config::default()
        };
        assert_eq!(preselect(&choices, &other_port), None);
        // Untouched defaults with Ollama missing: the first thing found.
        let only_cli = vec![Choice::cli(Provider::ClaudeCode)];
        assert_eq!(preselect(&only_cli, &Config::default()), Some(0));
        assert_eq!(Choice::cli(Provider::ClaudeCode).to_string(), "Claude Code");
        assert_eq!(Choice::ollama(ollama, "a").to_string(), "Ollama — a");
    }
}
