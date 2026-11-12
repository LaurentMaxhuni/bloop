use crate::{
    config::Config,
    model::{self, Chat, MAX_OUTPUT, MAX_PROMPT, MAX_TURNS, Turn},
};
use rusqlite::{Connection, OptionalExtension, params};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

/// Largest serialized settings (profiles included).
const MAX_SETTINGS: usize = 32 * 1024;
/// Largest serialized turns of one chat.
const MAX_HISTORY: usize = 16 * 1024 * 1024;
const MAX_TITLE: usize = 200;

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
    // `state.history` held the single conversation of bloop 0.4 and earlier; `load` moves it
    // into `chats` and it stays empty afterwards.
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS state (
        id INTEGER PRIMARY KEY CHECK(id = 1), settings TEXT NOT NULL, history TEXT NOT NULL
    );
    CREATE TABLE IF NOT EXISTS chatgpt_account (
        id INTEGER PRIMARY KEY CHECK(id = 1), data BLOB NOT NULL
    );
    CREATE TABLE IF NOT EXISTS chats (
        id INTEGER PRIMARY KEY, title TEXT NOT NULL, profile INTEGER NOT NULL,
        destination TEXT NOT NULL, updated INTEGER NOT NULL, turns TEXT NOT NULL
    );
    CREATE INDEX IF NOT EXISTS chats_updated ON chats(updated DESC);",
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

fn validate_chat(chat: &Chat) -> Result<(), String> {
    if chat.title.len() > MAX_TITLE || chat.destination.len() > 1024 {
        return Err("Chat details exceed supported limits.".into());
    }
    Ok(())
}

fn parse_turns(json: &str) -> Result<Vec<Turn>, String> {
    if json.len() > MAX_HISTORY {
        return Err("Saved chat exceeds supported limits.".into());
    }
    let turns: Vec<Turn> = serde_json::from_str(json).map_err(|e| e.to_string())?;
    validate_turns(&turns)?;
    Ok(turns)
}

fn chat_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Chat> {
    Ok(Chat {
        id: row.get(0)?,
        title: row.get(1)?,
        profile: row.get(2)?,
        destination: row.get(3)?,
        updated: row.get(4)?,
    })
}

const CHAT_COLUMNS: &str = "id, title, profile, destination, updated";

pub(crate) fn load_chatgpt() -> Result<Option<Vec<u8>>, String> {
    let path = data_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let connection = connect(&path)?;
    connection
        .query_row("SELECT data FROM chatgpt_account WHERE id = 1", [], |row| {
            row.get(0)
        })
        .optional()
        .map_err(|e| e.to_string())
}

pub(crate) fn save_chatgpt(protected: &[u8]) -> Result<(), String> {
    let connection = connect(&data_path()?)?;
    connection
        .execute(
            "INSERT INTO chatgpt_account(id, data) VALUES(1, ?1)
             ON CONFLICT(id) DO UPDATE SET data=excluded.data",
            [protected],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Everything bloop reads at start: settings, the saved chats (newest first) and the
/// newest chat's turns.
#[derive(Debug, Clone, Default)]
pub struct Loaded {
    pub config: Config,
    pub chats: Vec<Chat>,
    pub current: Option<(Chat, Vec<Turn>)>,
}

pub fn load(path: &Path) -> Result<Loaded, String> {
    let mut connection = connect(path)?;
    let row: Option<(String, String)> = connection
        .query_row(
            "SELECT settings, history FROM state WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let Some((settings, history)) = row else {
        return Ok(Loaded::default());
    };
    if settings.len() > MAX_SETTINGS {
        return Err("Saved data exceeds supported limits.".into());
    }
    let config: Config = serde_json::from_str(&settings).map_err(|e| e.to_string())?;
    config.validate()?;
    let legacy = parse_turns(&history)?;
    if !legacy.is_empty() {
        migrate_history(&mut connection, &config, &legacy, &history)?;
    }
    let chats = list(&connection)?;
    let current = match chats.first() {
        Some(chat) => Some((chat.clone(), turns_of(&connection, chat.id)?)),
        None => None,
    };
    Ok(Loaded {
        config,
        chats,
        current,
    })
}

/// Moves the single conversation of an older bloop into `chats`, in one transaction.
fn migrate_history(
    connection: &mut Connection,
    config: &Config,
    turns: &[Turn],
    json: &str,
) -> Result<(), String> {
    let chat = Chat {
        id: model::new_chat_id(0),
        title: model::title_for(&turns[0].user),
        profile: 0,
        destination: config.destination().key(),
        updated: model::unix_seconds(),
    };
    let transaction = connection.transaction().map_err(|e| e.to_string())?;
    upsert(&transaction, &chat, json)?;
    transaction
        .execute("UPDATE state SET history = '[]' WHERE id = 1", [])
        .map_err(|e| e.to_string())?;
    transaction.commit().map_err(|e| e.to_string())
}

fn list(connection: &Connection) -> Result<Vec<Chat>, String> {
    let mut statement = connection
        .prepare(&format!(
            "SELECT {CHAT_COLUMNS} FROM chats ORDER BY updated DESC, id DESC"
        ))
        .map_err(|e| e.to_string())?;
    statement
        .query_map([], chat_from_row)
        .and_then(Iterator::collect)
        .map_err(|e| e.to_string())
}

fn turns_of(connection: &Connection, id: i64) -> Result<Vec<Turn>, String> {
    let json: String = connection
        .query_row("SELECT turns FROM chats WHERE id = ?1", [id], |row| {
            row.get(0)
        })
        .map_err(|e| e.to_string())?;
    parse_turns(&json)
}

fn upsert(connection: &Connection, chat: &Chat, turns_json: &str) -> Result<(), String> {
    connection
        .execute(
            "INSERT INTO chats(id, title, profile, destination, updated, turns)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET title=excluded.title, profile=excluded.profile,
             destination=excluded.destination, updated=excluded.updated, turns=excluded.turns",
            params![
                chat.id,
                chat.title,
                chat.profile,
                chat.destination,
                chat.updated,
                turns_json
            ],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Saves settings and, when given, one chat, together in one transaction. A chat
/// without turns is not written.
pub fn save(path: &Path, config: &Config, chat: Option<(&Chat, &[Turn])>) -> Result<(), String> {
    config.validate()?;
    let settings = serde_json::to_string(config).map_err(|e| e.to_string())?;
    if settings.len() > MAX_SETTINGS {
        return Err("Saved data exceeds supported limits.".into());
    }
    let chat = match chat {
        Some((chat, turns)) if !turns.is_empty() => {
            validate_chat(chat)?;
            validate_turns(turns)?;
            let json = serde_json::to_string(turns).map_err(|e| e.to_string())?;
            if json.len() > MAX_HISTORY {
                return Err("Saved data exceeds supported limits.".into());
            }
            Some((chat, json))
        }
        _ => None,
    };
    let mut connection = connect(path)?;
    let transaction = connection.transaction().map_err(|e| e.to_string())?;
    transaction
        .execute(
            "INSERT INTO state(id, settings, history) VALUES(1, ?1, '[]')
        ON CONFLICT(id) DO UPDATE SET settings=excluded.settings",
            params![settings],
        )
        .map_err(|e| e.to_string())?;
    if let Some((chat, json)) = chat {
        upsert(&transaction, chat, &json)?;
    }
    transaction.commit().map_err(|e| e.to_string())
}

/// One saved chat with its turns.
pub fn load_chat(path: &Path, id: i64) -> Result<(Chat, Vec<Turn>), String> {
    let connection = connect(path)?;
    let chat = connection
        .query_row(
            &format!("SELECT {CHAT_COLUMNS} FROM chats WHERE id = ?1"),
            [id],
            chat_from_row,
        )
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or("That chat no longer exists.")?;
    let turns = turns_of(&connection, id)?;
    Ok((chat, turns))
}

pub fn rename_chat(path: &Path, id: i64, title: &str) -> Result<(), String> {
    let title = title.trim();
    if title.is_empty() || title.len() > MAX_TITLE {
        return Err("Chat names need 1–200 bytes.".into());
    }
    connect(path)?
        .execute(
            "UPDATE chats SET title = ?2 WHERE id = ?1",
            params![id, title],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn delete_chat(path: &Path, id: i64) -> Result<(), String> {
    connect(path)?
        .execute("DELETE FROM chats WHERE id = ?1", [id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Chats whose title or text contains `query` (ignoring case), newest first, each with an
/// excerpt of the first matching line. An empty query lists every chat.
pub fn search(path: &Path, query: &str) -> Result<Vec<(Chat, String)>, String> {
    let connection = connect(path)?;
    let needle = query.trim().to_lowercase();
    let mut statement = connection
        .prepare(&format!(
            "SELECT {CHAT_COLUMNS}, turns FROM chats ORDER BY updated DESC, id DESC"
        ))
        .map_err(|e| e.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok((chat_from_row(row)?, row.get::<_, String>(5)?))
        })
        .map_err(|e| e.to_string())?;
    let mut hits = Vec::new();
    for row in rows {
        let (chat, json) = row.map_err(|e| e.to_string())?;
        if needle.is_empty() {
            hits.push((chat, String::new()));
            continue;
        }
        // A damaged chat is skipped rather than failing the whole search.
        let Ok(turns) = parse_turns(&json) else {
            continue;
        };
        if let Some(snippet) = model::snippet(&turns, &needle) {
            hits.push((chat, snippet));
        } else if chat.title.to_lowercase().contains(&needle) {
            hits.push((chat, String::new()));
        }
    }
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "bloop-storage-{name}-{}.sqlite3",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        path
    }

    fn turn(user: &str, assistant: &str) -> Turn {
        Turn {
            user: user.into(),
            assistant: assistant.into(),
            complete: true,
        }
    }

    fn chat(id: i64, title: &str, updated: i64) -> Chat {
        Chat {
            id,
            title: title.into(),
            profile: 0,
            destination: "local".into(),
            updated,
        }
    }

    #[test]
    fn round_trip_and_failed_validation_preserves_saved_history() {
        let path = temp("round-trip");
        let turns = vec![turn("hello", "world")];
        let first = chat(1, "hello", 10);
        save(&path, &Config::default(), Some((&first, &turns))).unwrap();
        let invalid = Config {
            model: String::new(),
            ..Config::default()
        };
        assert!(save(&path, &invalid, Some((&first, &[]))).is_err());
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.config.model, Config::default().model);
        let (current, turns) = loaded.current.unwrap();
        assert_eq!(current, first);
        assert_eq!(turns[0].assistant, "world");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn oversized_serialized_settings_cannot_replace_readable_state() {
        let path = temp("boundary");
        save(&path, &Config::default(), None).unwrap();
        let config = Config {
            endpoint: format!("https://example.com/{}", "a".repeat(MAX_SETTINGS)),
            ..Config::default()
        };
        assert!(save(&path, &config, None).is_err());
        assert_eq!(
            load(&path).unwrap().config.endpoint,
            Config::default().endpoint
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn old_single_history_becomes_a_chat_once() {
        let path = temp("migrate");
        {
            let connection = connect(&path).unwrap();
            let settings = serde_json::to_string(&Config::default()).unwrap();
            let history =
                serde_json::to_string(&vec![turn("Old question\nmore", "answer")]).unwrap();
            connection
                .execute(
                    "INSERT INTO state(id, settings, history) VALUES(1, ?1, ?2)",
                    params![settings, history],
                )
                .unwrap();
        }
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.chats.len(), 1);
        let (chat, turns) = loaded.current.unwrap();
        assert_eq!(chat.title, "Old question");
        assert_eq!(chat.destination, "local");
        assert_eq!(turns[0].assistant, "answer");
        // A second load finds nothing left to migrate.
        assert_eq!(load(&path).unwrap().chats.len(), 1);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn chats_list_newest_first_and_can_be_renamed_searched_and_deleted() {
        let path = temp("chats");
        let config = Config::default();
        let older = chat(1, "Kettles", 100);
        let newer = chat(2, "Gardening", 200);
        save(
            &path,
            &config,
            Some((&older, &[turn("how do kettles work", "heat")])),
        )
        .unwrap();
        save(
            &path,
            &config,
            Some((&newer, &[turn("roses", "prune in March")])),
        )
        .unwrap();
        save(&path, &config, Some((&chat(3, "empty", 300), &[]))).unwrap();

        let loaded = load(&path).unwrap();
        let ids: Vec<i64> = loaded.chats.iter().map(|c| c.id).collect();
        assert_eq!(ids, vec![2, 1], "empty chats aren't saved");
        assert_eq!(loaded.current.unwrap().0.id, 2);

        let hits = search(&path, "MARCH").unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].1, "prune in March");
        assert_eq!(search(&path, "kettles").unwrap()[0].0.id, 1);
        assert_eq!(search(&path, "").unwrap().len(), 2);

        rename_chat(&path, 1, "  Tea  ").unwrap();
        assert_eq!(load_chat(&path, 1).unwrap().0.title, "Tea");
        assert!(rename_chat(&path, 1, " ").is_err());

        delete_chat(&path, 2).unwrap();
        assert!(load_chat(&path, 2).is_err());
        assert_eq!(load(&path).unwrap().chats.len(), 1);
        std::fs::remove_file(path).unwrap();
    }
}
