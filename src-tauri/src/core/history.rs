use rusqlite::{params, Connection, ErrorCode};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const HISTORY_FILE: &str = "transcriptions.sqlite3";
const SCHEMA_VERSION: i32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TranscriptionEntry {
    pub id: i64,
    pub text: String,
    pub created_at: String,
    pub duration_secs: i64,
}

pub struct TranscriptionStore {
    path: PathBuf,
}

impl TranscriptionStore {
    pub fn new(base_dir: &Path) -> Result<Self, String> {
        fs::create_dir_all(base_dir)
            .map_err(|e| format!("Failed to prepare transcription history directory: {}", e))?;
        let store = Self {
            path: base_dir.join(HISTORY_FILE),
        };
        store.initialize()?;
        Ok(store)
    }

    pub fn list_recent(&self, limit: i64) -> Result<Vec<TranscriptionEntry>, String> {
        self.search(None, limit)
    }

    pub fn search(&self, query: Option<&str>, limit: i64) -> Result<Vec<TranscriptionEntry>, String> {
        let connection = self.open()?;
        let normalized_query = query
            .map(|value| value.replace('\0', "").trim().to_string())
            .filter(|value| !value.is_empty());
        let clamped_limit = limit.clamp(1, 500);

        if let Some(query) = normalized_query {
            let mut statement = connection
                .prepare(
                    "SELECT id, text, created_at, duration_secs
                     FROM transcriptions
                     WHERE text LIKE ?1 ESCAPE '\\'
                     ORDER BY created_at DESC, id DESC
                     LIMIT ?2",
                )
                .map_err(classify_sqlite_error)?;
            let rows = statement
                .query_map(params![like_pattern(&query), clamped_limit], |row| {
                    Ok(TranscriptionEntry {
                        id: row.get(0)?,
                        text: row.get(1)?,
                        created_at: row.get(2)?,
                        duration_secs: row.get(3)?,
                    })
                })
                .map_err(classify_sqlite_error)?;

            rows.collect::<Result<Vec<_>, _>>()
                .map_err(classify_sqlite_error)
        } else {
            let mut statement = connection
                .prepare(
                    "SELECT id, text, created_at, duration_secs
                     FROM transcriptions
                     ORDER BY created_at DESC, id DESC
                     LIMIT ?1",
                )
                .map_err(classify_sqlite_error)?;
            let rows = statement
                .query_map(params![clamped_limit], |row| {
                    Ok(TranscriptionEntry {
                        id: row.get(0)?,
                        text: row.get(1)?,
                        created_at: row.get(2)?,
                        duration_secs: row.get(3)?,
                    })
                })
                .map_err(classify_sqlite_error)?;

            rows.collect::<Result<Vec<_>, _>>()
                .map_err(classify_sqlite_error)
        }
    }

    pub fn add(&self, text: &str, duration_secs: i64) -> Result<TranscriptionEntry, String> {
        let normalized = normalize_text(text)?;
        let connection = self.open()?;
        connection
            .execute(
                "INSERT INTO transcriptions (text, duration_secs) VALUES (?1, ?2)",
                params![normalized, duration_secs.max(0)],
            )
            .map_err(classify_sqlite_error)?;

        let id = connection.last_insert_rowid();
        self.get(id)?
            .ok_or_else(|| "Transcription history entry could not be saved".to_string())
    }

    pub fn delete(&self, id: i64) -> Result<(), String> {
        let connection = self.open()?;
        connection
            .execute("DELETE FROM transcriptions WHERE id = ?1", params![id])
            .map_err(classify_sqlite_error)?;
        Ok(())
    }

    pub fn get(&self, id: i64) -> Result<Option<TranscriptionEntry>, String> {
        let connection = self.open()?;
        let result = connection.query_row(
            "SELECT id, text, created_at, duration_secs FROM transcriptions WHERE id = ?1",
            params![id],
            |row| {
                Ok(TranscriptionEntry {
                    id: row.get(0)?,
                    text: row.get(1)?,
                    created_at: row.get(2)?,
                    duration_secs: row.get(3)?,
                })
            },
        );

        match result {
            Ok(entry) => Ok(Some(entry)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(classify_sqlite_error(e)),
        }
    }

    pub fn latest(&self) -> Result<Option<TranscriptionEntry>, String> {
        let connection = self.open()?;
        let result = connection.query_row(
            "SELECT id, text, created_at, duration_secs
             FROM transcriptions
             ORDER BY created_at DESC, id DESC
             LIMIT 1",
            [],
            |row| {
                Ok(TranscriptionEntry {
                    id: row.get(0)?,
                    text: row.get(1)?,
                    created_at: row.get(2)?,
                    duration_secs: row.get(3)?,
                })
            },
        );

        match result {
            Ok(entry) => Ok(Some(entry)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(classify_sqlite_error(e)),
        }
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
                "CREATE TABLE IF NOT EXISTS transcriptions (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    text TEXT NOT NULL,
                    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                    duration_secs INTEGER NOT NULL DEFAULT 0
                );
                CREATE INDEX IF NOT EXISTS idx_transcriptions_created_at
                    ON transcriptions(created_at DESC);",
            )
            .map_err(classify_sqlite_error)
    }

    fn open(&self) -> Result<Connection, String> {
        Connection::open(&self.path).map_err(classify_sqlite_error)
    }
}

fn like_pattern(query: &str) -> String {
    let escaped = query
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{}%", escaped)
}

fn normalize_text(text: &str) -> Result<String, String> {
    let normalized = text.replace('\0', "").trim().to_string();
    if normalized.is_empty() {
        return Err("Transcription text cannot be empty".to_string());
    }
    Ok(normalized)
}

fn classify_sqlite_error(error: rusqlite::Error) -> String {
    match &error {
        rusqlite::Error::SqliteFailure(failure, message) => match failure.code {
            ErrorCode::DatabaseCorrupt => "Transcription history database is corrupt. Move or delete transcriptions.sqlite3 from the WisprFlow app data folder, then restart the app.".to_string(),
            ErrorCode::DiskFull => {
                "Transcription history could not be saved because the disk is full.".to_string()
            }
            ErrorCode::CannotOpen => format!(
                "Transcription history database could not be opened: {}",
                message.clone().unwrap_or_else(|| error.to_string())
            ),
            _ => format!("Transcription history database error: {}", error),
        },
        _ => format!("Transcription history database error: {}", error),
    }
}
