use rusqlite::{params, Connection, ErrorCode};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const DICTIONARY_FILE: &str = "dictionary.sqlite3";
const SCHEMA_VERSION: i32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DictionaryTerm {
    pub id: i64,
    pub term: String,
    pub created_at: String,
}

pub struct DictionaryStore {
    path: PathBuf,
}

impl DictionaryStore {
    pub fn new(base_dir: &Path) -> Result<Self, String> {
        fs::create_dir_all(base_dir)
            .map_err(|e| format!("Failed to prepare dictionary directory: {}", e))?;
        let store = Self {
            path: base_dir.join(DICTIONARY_FILE),
        };
        store.initialize()?;
        Ok(store)
    }

    pub fn list_terms(&self) -> Result<Vec<DictionaryTerm>, String> {
        let connection = self.open()?;
        let mut statement = connection
            .prepare("SELECT id, term, created_at FROM dictionary_terms ORDER BY lower(term)")
            .map_err(classify_sqlite_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok(DictionaryTerm {
                    id: row.get(0)?,
                    term: row.get(1)?,
                    created_at: row.get(2)?,
                })
            })
            .map_err(classify_sqlite_error)?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(classify_sqlite_error)
    }

    pub fn add_term(&self, term: &str) -> Result<DictionaryTerm, String> {
        let normalized = normalize_term(term)?;
        let connection = self.open()?;
        connection
            .execute(
                "INSERT OR IGNORE INTO dictionary_terms (term) VALUES (?1)",
                params![normalized],
            )
            .map_err(classify_sqlite_error)?;

        self.get_by_term(&normalized)?
            .ok_or_else(|| "Dictionary term could not be saved".to_string())
    }

    pub fn remove_term(&self, id: i64) -> Result<(), String> {
        let connection = self.open()?;
        connection
            .execute("DELETE FROM dictionary_terms WHERE id = ?1", params![id])
            .map_err(classify_sqlite_error)?;
        Ok(())
    }

    pub fn prompt_terms(&self) -> Result<Vec<String>, String> {
        Ok(self
            .list_terms()?
            .into_iter()
            .map(|term| term.term)
            .collect())
    }

    fn initialize(&self) -> Result<(), String> {
        let connection = self.open()?;
        connection
            .pragma_update(None, "journal_mode", "WAL")
            .map_err(classify_sqlite_error)?;
        connection
            .pragma_update(None, "user_version", SCHEMA_VERSION)
            .map_err(classify_sqlite_error)?;
        connection
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS dictionary_terms (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    term TEXT NOT NULL UNIQUE COLLATE NOCASE,
                    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                );",
            )
            .map_err(classify_sqlite_error)
    }

    fn get_by_term(&self, term: &str) -> Result<Option<DictionaryTerm>, String> {
        let connection = self.open()?;
        let result = connection.query_row(
            "SELECT id, term, created_at FROM dictionary_terms WHERE term = ?1 COLLATE NOCASE",
            params![term],
            |row| {
                Ok(DictionaryTerm {
                    id: row.get(0)?,
                    term: row.get(1)?,
                    created_at: row.get(2)?,
                })
            },
        );

        match result {
            Ok(term) => Ok(Some(term)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(classify_sqlite_error(e)),
        }
    }

    fn open(&self) -> Result<Connection, String> {
        Connection::open(&self.path).map_err(classify_sqlite_error)
    }
}

fn normalize_term(term: &str) -> Result<String, String> {
    let normalized = term
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace('\0', "")
        .trim()
        .to_string();

    if normalized.is_empty() {
        return Err("Dictionary term cannot be empty".to_string());
    }

    if normalized.len() > 120 {
        return Err("Dictionary term must be 120 characters or fewer".to_string());
    }

    Ok(normalized)
}

fn classify_sqlite_error(error: rusqlite::Error) -> String {
    match &error {
        rusqlite::Error::SqliteFailure(failure, message) => match failure.code {
            ErrorCode::DatabaseCorrupt => "Dictionary database is corrupt. Move or delete dictionary.sqlite3 from the wisprflow app data folder, then restart the app.".to_string(),
            ErrorCode::DiskFull => "Dictionary term could not be saved because the disk is full.".to_string(),
            ErrorCode::CannotOpen => format!(
                "Dictionary database could not be opened: {}",
                message.clone().unwrap_or_else(|| error.to_string())
            ),
            _ => format!("Dictionary database error: {}", error),
        },
        _ => format!("Dictionary database error: {}", error),
    }
}
