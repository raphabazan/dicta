use rusqlite::{Connection, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionEntry {
    pub id: Option<i64>,
    pub text: String,
    pub timestamp: i64, // Unix timestamp in milliseconds
    pub duration_ms: Option<i64>,
    pub model: Option<String>,
    pub cost_cents: Option<i64>, // hundredths of a cent for precision
    pub mode: Option<String>,    // "transcription" or "prompt"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMessage {
    pub role: String,    // "user" or "assistant"
    pub content: String,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatsData {
    pub total_words: i64,
    pub total_transcriptions: i64,
    pub total_duration_ms: i64,
    pub total_cost_cents: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingQueueItem {
    pub id: i64,
    pub mode: String,              // 'whisper-transcribe' | 'whisper-prompt' | 'text-prompt' | 'realtime-prompt'
    pub audio_path: Option<String>, // absolute path to WAV file (only for whisper-transcribe)
    pub prompt_text: Option<String>, // text for send_prompt retry
    pub model: String,
    pub created_at: i64,
    pub retry_count: i64,
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

        // Schema migration v1: add stats columns to transcriptions
        let schema_version: i64 = conn
            .query_row(
                "SELECT COALESCE(
                    (SELECT CAST(value AS INTEGER) FROM settings WHERE key = 'schema_version'),
                    0
                )",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        if schema_version < 1 {
            conn.execute("ALTER TABLE transcriptions ADD COLUMN duration_ms INTEGER", [])?;
            conn.execute("ALTER TABLE transcriptions ADD COLUMN model TEXT", [])?;
            conn.execute("ALTER TABLE transcriptions ADD COLUMN cost_cents INTEGER", [])?;
            conn.execute("ALTER TABLE transcriptions ADD COLUMN mode TEXT DEFAULT 'transcription'", [])?;
            conn.execute(
                "INSERT OR REPLACE INTO settings (key, value) VALUES ('schema_version', '1')",
                [],
            )?;
            println!("📦 Database migrated to schema version 1 (added stats columns)");
        }

        if schema_version < 2 {
            conn.execute(
                "CREATE TABLE IF NOT EXISTS pending_queue (
                    id          INTEGER PRIMARY KEY AUTOINCREMENT,
                    mode        TEXT    NOT NULL,
                    audio_path  TEXT,
                    prompt_text TEXT,
                    model       TEXT    NOT NULL,
                    created_at  INTEGER NOT NULL,
                    retry_count INTEGER NOT NULL DEFAULT 0
                )",
                [],
            )?;
            conn.execute(
                "INSERT OR REPLACE INTO settings (key, value) VALUES ('schema_version', '2')",
                [],
            )?;
            println!("📦 Database migrated to schema version 2 (added pending_queue)");
        }

        println!("✅ Database initialized");

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Save a new transcription to the database
    pub fn save_transcription(
        &self,
        text: &str,
        timestamp: i64,
        duration_ms: Option<i64>,
        model: Option<&str>,
        cost_cents: Option<i64>,
        mode: Option<&str>,
    ) -> Result<i64> {
        let conn = self.conn.lock().unwrap();

        conn.execute(
            "INSERT INTO transcriptions (text, timestamp, duration_ms, model, cost_cents, mode)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![text, timestamp, duration_ms, model, cost_cents, mode],
        )?;

        let id = conn.last_insert_rowid();
        println!("💾 Saved transcription to DB (id: {}, model: {:?}, cost: {:?})", id, model, cost_cents);

        Ok(id)
    }

    /// Load all transcriptions ordered by timestamp (most recent first)
    pub fn load_transcriptions(&self) -> Result<Vec<TranscriptionEntry>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT id, text, timestamp, duration_ms, model, cost_cents, mode
             FROM transcriptions ORDER BY timestamp DESC",
        )?;

        let entries = stmt
            .query_map([], |row| {
                Ok(TranscriptionEntry {
                    id: Some(row.get(0)?),
                    text: row.get(1)?,
                    timestamp: row.get(2)?,
                    duration_ms: row.get(3)?,
                    model: row.get(4)?,
                    cost_cents: row.get(5)?,
                    mode: row.get(6)?,
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
             ) ORDER BY timestamp ASC",
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

    /// Clear all transcriptions
    pub fn clear_transcriptions(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM transcriptions", [])?;
        println!("🗑️ All transcriptions cleared");
        Ok(())
    }

    /// Get statistics for a date range
    pub fn get_stats(&self, from_ts: i64, to_ts: i64) -> Result<StatsData> {
        let conn = self.conn.lock().unwrap();

        let total_transcriptions: i64 = conn.query_row(
            "SELECT COUNT(*) FROM transcriptions WHERE timestamp >= ?1 AND timestamp <= ?2",
            rusqlite::params![from_ts, to_ts],
            |row| row.get(0),
        )?;

        let total_duration_ms: i64 = conn.query_row(
            "SELECT COALESCE(SUM(duration_ms), 0) FROM transcriptions
             WHERE timestamp >= ?1 AND timestamp <= ?2",
            rusqlite::params![from_ts, to_ts],
            |row| row.get(0),
        )?;

        let total_cost_cents: i64 = conn.query_row(
            "SELECT COALESCE(SUM(cost_cents), 0) FROM transcriptions
             WHERE timestamp >= ?1 AND timestamp <= ?2",
            rusqlite::params![from_ts, to_ts],
            |row| row.get(0),
        )?;

        // Word count: load texts and count in Rust
        let mut stmt = conn.prepare(
            "SELECT text FROM transcriptions WHERE timestamp >= ?1 AND timestamp <= ?2",
        )?;
        let texts: Vec<String> = stmt
            .query_map(rusqlite::params![from_ts, to_ts], |row| row.get(0))?
            .collect::<Result<Vec<_>>>()?;
        let total_words: i64 = texts
            .iter()
            .map(|t| t.split_whitespace().count() as i64)
            .sum();

        Ok(StatsData {
            total_words,
            total_transcriptions,
            total_duration_ms,
            total_cost_cents,
        })
    }

    // --- Pending Queue methods ---

    pub fn enqueue_item(
        &self,
        mode: &str,
        audio_path: Option<&str>,
        prompt_text: Option<&str>,
        model: &str,
        created_at: i64,
    ) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO pending_queue (mode, audio_path, prompt_text, model, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![mode, audio_path, prompt_text, model, created_at],
        )?;
        let id = conn.last_insert_rowid();
        println!("📋 Queued item id={} mode={} model={}", id, mode, model);
        Ok(id)
    }

    pub fn load_queue(&self) -> Result<Vec<PendingQueueItem>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, mode, audio_path, prompt_text, model, created_at, retry_count
             FROM pending_queue ORDER BY created_at ASC",
        )?;
        let items = stmt
            .query_map([], |row| {
                Ok(PendingQueueItem {
                    id: row.get(0)?,
                    mode: row.get(1)?,
                    audio_path: row.get(2)?,
                    prompt_text: row.get(3)?,
                    model: row.get(4)?,
                    created_at: row.get(5)?,
                    retry_count: row.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>>>()?;
        Ok(items)
    }

    pub fn delete_queue_item(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM pending_queue WHERE id = ?1", [id])?;
        println!("🗑️ Deleted queue item id={}", id);
        Ok(())
    }

    pub fn count_queue(&self) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM pending_queue",
            [],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    pub fn increment_retry_count(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE pending_queue SET retry_count = retry_count + 1 WHERE id = ?1",
            [id],
        )?;
        Ok(())
    }
}
