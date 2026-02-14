use rusqlite::{Connection, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionEntry {
    pub id: Option<i64>,
    pub text: String,
    pub timestamp: i64, // Unix timestamp in milliseconds
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMessage {
    pub role: String,    // "user" or "assistant"
    pub content: String,
    pub timestamp: i64,
}

pub struct Database {
    conn: Arc<Mutex<Connection>>,
}

impl Database {
    /// Initialize database with schema
    pub fn new(db_path: PathBuf) -> Result<Self> {
        let conn = Connection::open(db_path)?;

        // Create transcriptions table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS transcriptions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                text TEXT NOT NULL,
                timestamp INTEGER NOT NULL
            )",
            [],
        )?;

        // Create index on timestamp for faster queries
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_timestamp ON transcriptions(timestamp DESC)",
            [],
        )?;

        // Create settings table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )",
            [],
        )?;

        // Create conversation history table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS conversation_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                timestamp INTEGER NOT NULL
            )",
            [],
        )?;

        println!("✅ Database initialized");

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Save a new transcription to the database
    pub fn save_transcription(&self, text: &str, timestamp: i64) -> Result<i64> {
        let conn = self.conn.lock().unwrap();

        conn.execute(
            "INSERT INTO transcriptions (text, timestamp) VALUES (?1, ?2)",
            [text, &timestamp.to_string()],
        )?;

        let id = conn.last_insert_rowid();
        println!("💾 Saved transcription to DB (id: {})", id);

        Ok(id)
    }

    /// Load all transcriptions ordered by timestamp (most recent first)
    pub fn load_transcriptions(&self) -> Result<Vec<TranscriptionEntry>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT id, text, timestamp FROM transcriptions ORDER BY timestamp DESC"
        )?;

        let entries = stmt
            .query_map([], |row| {
                Ok(TranscriptionEntry {
                    id: Some(row.get(0)?),
                    text: row.get(1)?,
                    timestamp: row.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>>>()?;

        println!("📚 Loaded {} transcriptions from DB", entries.len());

        Ok(entries)
    }

    /// Delete a transcription by ID
    pub fn delete_transcription(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();

        conn.execute("DELETE FROM transcriptions WHERE id = ?1", [id])?;

        println!("🗑️ Deleted transcription (id: {})", id);

        Ok(())
    }

    /// Get total count of transcriptions
    pub fn count_transcriptions(&self) -> Result<i64> {
        let conn = self.conn.lock().unwrap();

        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM transcriptions",
            [],
            |row| row.get(0),
        )?;

        Ok(count)
    }

    /// Save a setting
    pub fn save_setting(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();

        conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
            [key, value],
        )?;

        Ok(())
    }

    /// Load a setting
    pub fn load_setting(&self, key: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();

        let result = conn.query_row(
            "SELECT value FROM settings WHERE key = ?1",
            [key],
            |row| row.get(0),
        );

        match result {
            Ok(value) => Ok(Some(value)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Append a message to conversation history
    pub fn append_conversation(&self, role: &str, content: &str, timestamp: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO conversation_history (role, content, timestamp) VALUES (?1, ?2, ?3)",
            rusqlite::params![role, content, timestamp],
        )?;
        Ok(())
    }

    /// Load the last N pairs (user + assistant) in chronological order
    pub fn load_conversation_history(&self, max_pairs: usize) -> Result<Vec<ConversationMessage>> {
        let conn = self.conn.lock().unwrap();
        let limit = (max_pairs * 2) as i64;

        let mut stmt = conn.prepare(
            "SELECT role, content, timestamp FROM (
                SELECT role, content, timestamp FROM conversation_history
                ORDER BY timestamp DESC LIMIT ?1
             ) ORDER BY timestamp ASC"
        )?;

        let messages = stmt
            .query_map(rusqlite::params![limit], |row| {
                Ok(ConversationMessage {
                    role: row.get(0)?,
                    content: row.get(1)?,
                    timestamp: row.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>>>()?;

        Ok(messages)
    }

    /// Get the timestamp of the most recent conversation message
    pub fn last_conversation_timestamp(&self) -> Result<Option<i64>> {
        let conn = self.conn.lock().unwrap();
        let result = conn.query_row(
            "SELECT MAX(timestamp) FROM conversation_history",
            [],
            |row| row.get::<_, Option<i64>>(0),
        );
        match result {
            Ok(ts) => Ok(ts),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Clear all conversation history
    pub fn clear_conversation_history(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM conversation_history", [])?;
        println!("🗑️ Conversation history cleared");
        Ok(())
    }
}
