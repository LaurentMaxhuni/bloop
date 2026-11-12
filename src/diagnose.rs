//! User-requested connection checks. Never sends a prompt or starts a CLI.

use std::{path::PathBuf, time::Duration};

use crate::{
    chatgpt,
    cli::{self, Kind},
    config::{Config, Provider, validate_endpoint},
    provider::{self, Error},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub ok: bool,
    pub summary: String,
    pub fix: Option<String>,
}

impl Report {
    fn success(summary: impl Into<String>) -> Self {
        Self {
            ok: true,
            summary: summary.into(),
            fix: None,
        }
    }

    fn failure(summary: impl Into<String>, fix: impl Into<String>) -> Self {
        Self {
            ok: false,
            summary: summary.into(),
            fix: Some(fix.into()),
        }
    }
}

/// One bounded model-list GET; ChatGPT may first refresh its saved credentials.
/// The report deadline includes authentication. The existing refresh operation is
/// cancellation-safe and may finish saving a rotated token after this returns.
pub async fn test_connection(config: &Config) -> Report {
    tokio::time::timeout(Duration::from_secs(10), check(config))
        .await
        .unwrap_or_else(|_| timed_out())
}

async fn check(config: &Config) -> Report {
    if let Some(kind) = Kind::of(config.provider) {
        return match tokio::task::spawn_blocking(move || cli::find(kind)).await {
            Ok(path) => cli_report(kind, path),
            Err(_) => Report::failure(
                "Could not check the CLI path.",
                "Try again, or restart bloop.",
            ),
        };
    }
    let models = if config.provider == Provider::ChatGpt {
        let status = match chatgpt::status().await {
            Ok(status) => status,
            Err(_) => {
                return Report::failure(
                    "Could not read the saved ChatGPT account.",
                    "Sign in again under Settings → Model.",
                );
            }
        };
        if let Some(report) = account_problem(&status) {
            return report;
        }
        provider::chatgpt_models()
            .await
            .map(|models| models.into_iter().map(|model| model.slug).collect())
    } else {
        if let Err(reason) = validate_endpoint(&config.endpoint) {
            return Report::failure(
                "The provider URL is invalid.",
                format!("Check the base URL in Settings → Model. {reason}"),
            );
        }
        provider::list_models(config).await
    };
    match models {
        Ok(models) => model_report(config, &models),
        Err(error) => failure(config.provider, error),
    }
}

fn account_problem(status: &chatgpt::AccountStatus) -> Option<Report> {
    if !status.signed_in {
        Some(Report::failure(
            "ChatGPT is not signed in.",
            "Sign in to ChatGPT under Settings → Model.",
        ))
    } else if !status.plan_enabled {
        Some(Report::failure(
            "ChatGPT is signed in, but plan usage is not enabled.",
            "Enable ChatGPT plan usage under Settings → Model with an eligible account.",
        ))
    } else {
        None
    }
}

fn cli_report(kind: Kind, path: Option<PathBuf>) -> Report {
    match path {
        Some(path) => Report::success(format!(
            "{} found at {}. Sign-in was not checked.",
            kind.name(),
            path.display()
        )),
        None => Report::failure(
            format!("{} was not found on PATH.", kind.name()),
            match kind {
                Kind::Claude => {
                    "Install Claude Code (winget install Anthropic.ClaudeCode), run claude to sign in, then restart bloop to refresh PATH."
                }
                Kind::Codex => {
                    "Install Codex CLI (pnpm add -g @openai/codex), run codex login, then restart bloop to refresh PATH."
                }
            },
        ),
    }
}

fn model_report(config: &Config, models: &[String]) -> Report {
    let model = config.model.trim();
    let found = models.iter().any(|name| {
        name == model
            || (config.provider == Provider::Ollama
                && !model.contains(':')
                && name == &format!("{model}:latest"))
    });
    if found || (config.provider == Provider::ChatGpt && model.is_empty() && !models.is_empty()) {
        return Report::success(format!(
            "Connected to {}. Model {} is available.",
            config.provider,
            if model.is_empty() { &models[0] } else { model }
        ));
    }
    let available = if models.is_empty() {
        "The server listed no models.".to_owned()
    } else {
        format!(
            "Available models: {}{}.",
            models
                .iter()
                .take(5)
                .cloned()
                .collect::<Vec<_>>()
                .join(", "),
            if models.len() > 5 { ", …" } else { "" }
        )
    };
    Report::failure(
        if model.is_empty() {
            format!(
                "Connected to {}, but no model is selected. {available}",
                config.provider
            )
        } else {
            format!(
                "Connected to {}, but model \"{model}\" is not listed. {available}",
                config.provider
            )
        },
        "Choose an available model in Settings → Model, or load/download the configured model on the server.",
    )
}

fn timed_out() -> Report {
    Report::failure(
        "The connection check timed out.",
        "Check that the server is running and reachable, then try again. Check the host, port, and firewall if it keeps timing out.",
    )
}

fn failure(provider: Provider, error: Error) -> Report {
    match error {
        Error::Http {
            status: 401 | 403, ..
        } if provider != Provider::ChatGpt => Report::failure(
            "The API key is missing, rejected, or does not have access.",
            "Set or replace the key in Settings → Model → API key or BLOOP_API_KEY. For Unsloth Studio, use the key printed when the model loads.",
        ),
        Error::Http { status: 404, .. } => Report::failure(
            "The model-list URL was not found (HTTP 404).",
            "Check the base URL path in Settings → Model. OpenAI-compatible base URLs usually end in /v1; Ollama normally uses http://127.0.0.1:11434 without /v1.",
        ),
        Error::Network(error) => network_failure(provider, &error),
        Error::Http { message, .. } | Error::Protocol(message) => Report::failure(
            message,
            if provider == Provider::ChatGpt {
                "Check your ChatGPT sign-in and plan access under Settings → Model, or try again later."
            } else {
                "Check the server logs and base URL. The server must expose a compatible model-list endpoint."
            },
        ),
    }
}

fn network_failure(provider: Provider, error: &reqwest::Error) -> Report {
    if error.is_timeout() {
        return timed_out();
    }
    // Inspect the source chain before its details are lost to a display string.
    // Never include remote URLs, credentials or arbitrary TLS messages in the report.
    let mut cause: Option<&(dyn std::error::Error + 'static)> = Some(error);
    let mut refused = false;
    let mut tls = false;
    while let Some(error) = cause {
        refused |= error
            .downcast_ref::<std::io::Error>()
            .is_some_and(|e| e.kind() == std::io::ErrorKind::ConnectionRefused);
        let text = error.to_string().to_ascii_lowercase();
        tls |= [
            "tls",
            "ssl",
            "certificate",
            "cert ",
            "certvalid",
            "invalid peer",
        ]
        .iter()
        .any(|word| text.contains(word));
        cause = error.source();
    }
    if refused {
        return Report::failure(
            "Connection refused: nothing is listening at that address and port.",
            match provider {
                Provider::Ollama => "Start Ollama, or check the port in Settings → Model.",
                Provider::Unsloth => {
                    "Start Unsloth Studio and load a model, or check the port in Settings → Model."
                }
                _ => "Start the model server, or check the host and port in Settings → Model.",
            },
        );
    }
    if tls {
        return Report::failure(
            "Could not establish a secure TLS connection.",
            "Check the server's HTTPS certificate, hostname, and your computer's clock. Use a valid trusted certificate; do not disable certificate verification.",
        );
    }
    Report::failure(
        "Could not reach the provider.",
        "Check the server address, network connection, DNS, and firewall, then try again.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chatgpt_requires_sign_in_and_plan_before_network() {
        let mut status = chatgpt::AccountStatus::default();
        assert!(
            account_problem(&status)
                .unwrap()
                .summary
                .contains("not signed in")
        );
        status.signed_in = true;
        assert!(
            account_problem(&status)
                .unwrap()
                .summary
                .contains("not enabled")
        );
        status.plan_enabled = true;
        assert!(account_problem(&status).is_none());
    }

    #[test]
    fn cli_reports_discovery_without_claiming_authentication() {
        for kind in [Kind::Claude, Kind::Codex] {
            let missing = cli_report(kind, None);
            assert!(!missing.ok);
            assert!(missing.fix.unwrap().contains("Install"));
            let found = cli_report(kind, Some(PathBuf::from("C:/tools/cli.exe")));
            assert!(found.ok);
            assert!(found.summary.contains("C:/tools/cli.exe"));
            assert!(found.summary.contains("Sign-in was not checked"));
        }
    }
}
