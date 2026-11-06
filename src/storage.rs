use crate::{
    config::Config,
    model::{MAX_OUTPUT, MAX_PROMPT, MAX_TURNS, Turn},
};
use rusqlite::{Connection, OptionalExtension, params};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

pub fn data_path() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("BLOOP_DATA_DIR") {
        return Ok(PathBuf::from(path).join("bloop.sqlite3"));
    }
    #[cfg(windows)]
    let root = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    #[cfg(not(windows))]
    let root = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".local/share")));
    root.map(|root| root.join("bloop/bloop.sqlite3"))
        .ok_or_else(|| "Could not locate the local application data directory.".into())
}

fn connect(path: &Path) -> Result<Connection, String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let connection = Connection::open(path).map_err(|e| e.to_string())?;
    connection
        .busy_timeout(Duration::from_secs(3))
        .map_err(|e| e.to_string())?;
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS state (
        id INTEGER PRIMARY KEY CHECK(id = 1), settings TEXT NOT NULL, history TEXT NOT NULL
    );",
        )
        .map_err(|e| e.to_string())?;
    Ok(connection)
}

fn validate_turns(turns: &[Turn]) -> Result<(), String> {
    if turns.len() > MAX_TURNS
        || turns
            .iter()
            .any(|t| t.user.len() > MAX_PROMPT || t.assistant.len() > MAX_OUTPUT)
    {
        return Err("Stored conversation exceeds supported limits.".into());
    }
    Ok(())
}

pub fn load(path: &Path) -> Result<(Config, Vec<Turn>), String> {
    let connection = connect(path)?;
    let row: Option<(String, String)> = connection
        .query_row(
            "SELECT settings, history FROM state WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    match row {
        None => Ok((Config::default(), Vec::new())),
        Some((settings, history)) => {
            if settings.len() > 8192 || history.len() > 16 * 1024 * 1024 {
                return Err("Saved data exceeds supported limits.".into());
            }
            let config: Config = serde_json::from_str(&settings).map_err(|e| e.to_string())?;
            let turns: Vec<Turn> = serde_json::from_str(&history).map_err(|e| e.to_string())?;
            config.validate()?;
            validate_turns(&turns)?;
            Ok((config, turns))
        }
    }
}

pub fn save(path: &Path, config: &Config, turns: &[Turn]) -> Result<(), String> {
    config.validate()?;
    validate_turns(turns)?;
    let settings = serde_json::to_string(config).map_err(|e| e.to_string())?;
    let history = serde_json::to_string(turns).map_err(|e| e.to_string())?;
    let connection = connect(path)?;
    // A single statement atomically replaces settings and history together.
    connection
        .execute(
            "INSERT INTO state(id, settings, history) VALUES(1, ?1, ?2)
        ON CONFLICT(id) DO UPDATE SET settings=excluded.settings, history=excluded.history",
            params![settings, history],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trip_and_failed_validation_preserves_saved_history() {
        let path =
            std::env::temp_dir().join(format!("bloop-storage-{}.sqlite3", std::process::id()));
        let turns = vec![Turn {
            user: "hello".into(),
            assistant: "world".into(),
            complete: true,
        }];
        save(&path, &Config::default(), &turns).unwrap();
        let invalid = Config {
            model: String::new(),
            ..Config::default()
        };
        assert!(save(&path, &invalid, &[]).is_err());
        let (settings, loaded) = load(&path).unwrap();
        assert_eq!(settings.model, Config::default().model);
        assert_eq!(loaded[0].assistant, "world");
        std::fs::remove_file(path).unwrap();
    }
}
