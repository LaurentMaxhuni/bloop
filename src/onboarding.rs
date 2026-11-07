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

/// Index of the choice to preselect: the current provider and model if found, else the
/// current provider, else the first.
pub fn preselect(choices: &[Choice], current: &Config) -> usize {
    choices
        .iter()
        .position(|c| c.provider == current.provider && c.model == current.model)
        .or_else(|| choices.iter().position(|c| c.provider == current.provider))
        .unwrap_or(0)
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
        let choices = vec![
            Choice::ollama("http://127.0.0.1:11434", "a"),
            Choice::ollama("http://127.0.0.1:11434", "qwen3:4b"),
            Choice::cli(Provider::Codex),
        ];
        assert_eq!(preselect(&choices, &Config::default()), 1);
        let codex = Config {
            provider: Provider::Codex,
            ..Config::default()
        };
        assert_eq!(preselect(&choices, &codex), 2);
        let unsloth = Config {
            provider: Provider::Unsloth,
            ..Config::default()
        };
        assert_eq!(preselect(&choices, &unsloth), 0);
        assert_eq!(Choice::cli(Provider::ClaudeCode).to_string(), "Claude Code");
        assert_eq!(
            Choice::ollama("http://127.0.0.1:11434", "a").to_string(),
            "Ollama — a"
        );
    }
}
