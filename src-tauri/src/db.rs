use rusqlite::{Connection, OptionalExtension, Result};
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    fn path() -> PathBuf {
        std::env::temp_dir().join(format!(
            "dicta-db-{}-{}-{}.sqlite",
            std::process::id(),
            crate::now_ms(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ))
    }
    #[test]
    fn fresh_and_repeat_migrations_preserve_history() {
        let file = path();
        let database = Database::new(file.clone()).unwrap();
        database
            .save_transcription("teste", 1, None, None, None, None)
            .unwrap();
        drop(database);
        let database = Database::new(file.clone()).unwrap();
        assert_eq!(
            database.load_setting("schema_version").unwrap().as_deref(),
            Some("4")
        );
        assert_eq!(
            database.load_latest_transcription().unwrap().unwrap().text,
            "teste"
        );
        drop(database);
        std::fs::remove_file(file).unwrap();
    }
    #[test]
    fn migrates_partial_legacy_schema_idempotently() {
        for version in 0..=3 {
            let file = path();
            let conn = Connection::open(&file).unwrap();
            conn.execute_batch("CREATE TABLE transcriptions(id INTEGER PRIMARY KEY,text TEXT NOT NULL,timestamp INTEGER NOT NULL,duration_ms INTEGER);CREATE TABLE settings(key TEXT PRIMARY KEY,value TEXT NOT NULL);").unwrap();
            if version >= 1 {
                for (name, kind) in [
                    ("model", "TEXT"),
                    ("cost_cents", "INTEGER"),
                    ("mode", "TEXT"),
                ] {
                    ensure_column(&conn, "transcriptions", name, kind).unwrap();
                }
            }
            if version >= 2 {
                conn.execute_batch("CREATE TABLE pending_queue(id INTEGER PRIMARY KEY,mode TEXT NOT NULL,audio_path TEXT,prompt_text TEXT,model TEXT NOT NULL,created_at INTEGER NOT NULL,retry_count INTEGER NOT NULL DEFAULT 0);").unwrap();
            }
            // Mimics interruption of the old non-transactional v3 migration.
            if version == 2 {
                ensure_column(&conn, "pending_queue", "last_error", "TEXT").unwrap();
            }
            if version == 3 {
                ensure_column(&conn, "pending_queue", "last_error", "TEXT").unwrap();
                ensure_column(
                    &conn,
                    "pending_queue",
                    "auto_retry_disabled",
                    "INTEGER NOT NULL DEFAULT 0",
                )
                .unwrap();
            }
            conn.execute(
                "INSERT INTO settings VALUES ('schema_version',?1)",
                [version.to_string()],
            )
            .unwrap();
            drop(conn);
            let database = Database::new(file.clone()).unwrap();
            assert_eq!(
                database.load_setting("schema_version").unwrap().as_deref(),
                Some("4")
            );
            drop(database);
            let again = Database::new(file.clone()).unwrap();
            drop(again);
            std::fs::remove_file(file).unwrap();
        }
    }
    #[test]
    fn queue_claim_is_exclusive_and_failures_release_it() {
        let database = Database::new(PathBuf::from(":memory:")).unwrap();
        let id = database
            .enqueue_item("text-prompt", None, Some("teste"), "gpt-5.4", 1)
            .unwrap();
        assert!(database.claim_queue_item(id, true).unwrap());
        assert!(!database.claim_queue_item(id, true).unwrap());
        database
            .record_queue_failure(id, "insufficient_quota", true)
            .unwrap();
        assert!(!database.claim_queue_item(id, false).unwrap());
        assert!(database.claim_queue_item(id, true).unwrap());
    }
    #[test]
    fn result_is_saved_once_and_history_is_paginated() {
        let database = Database::new(PathBuf::from(":memory:")).unwrap();
        for _ in 0..2 {
            database
                .save_result_once(
                    "session-1",
                    "teste",
                    Some(1000),
                    "gpt-transcribe",
                    "transcription",
                )
                .unwrap();
        }
        assert_eq!(database.count_transcriptions().unwrap(), 1);
        assert_eq!(database.load_history_page(1, 0).unwrap().len(), 1);
        assert!(database.load_history_page(1, 1).unwrap().is_empty());
    }
    #[test]
    fn startup_preserves_unfinished_sessions_without_delivery() {
        let database = Database::new(PathBuf::from(":memory:")).unwrap();
        let session = SessionRecord {
            id: "1-2-3".into(),
            status: "recording".into(),
            created_at: 1,
            duration_ms: 0,
            config: "{}".into(),
            text: String::new(),
            result: String::new(),
        };
        database.create_session(&session).unwrap();
        database.recover_interrupted_sessions().unwrap();
        let rows = database.load_recoverable_sessions().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, "recoverable");
        assert_eq!(database.count_transcriptions().unwrap(), 0);
        assert!(database.claim_session(&session.id).unwrap());
        assert!(!database.claim_session(&session.id).unwrap());
    }
    #[test]
    fn failed_migration_rolls_back_added_columns() {
        let file = path();
        let conn = Connection::open(&file).unwrap();
        conn.execute_batch("CREATE TABLE transcriptions(id INTEGER PRIMARY KEY,text TEXT NOT NULL,timestamp INTEGER NOT NULL,source_id TEXT);INSERT INTO transcriptions VALUES(1,'a',1,'same');INSERT INTO transcriptions VALUES(2,'b',2,'same');CREATE TABLE settings(key TEXT PRIMARY KEY,value TEXT NOT NULL);").unwrap();
        drop(conn);
        assert!(Database::new(file.clone()).is_err());
        let conn = Connection::open(&file).unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('transcriptions') WHERE name='duration_ms'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
        drop(conn);
        std::fs::remove_file(file).unwrap();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMessage {
    pub role: String, // "user" or "assistant"
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
    pub mode: String, // 'whisper-transcribe' | 'whisper-prompt' | 'text-prompt' | 'realtime-prompt'
    pub audio_path: Option<String>, // absolute path to WAV file (only for whisper-transcribe)
    pub prompt_text: Option<String>, // text for send_prompt retry
    pub model: String,
    pub created_at: i64,
    pub retry_count: i64,
    pub last_error: Option<String>,
    pub auto_retry_disabled: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionRecord {
    pub id: String,
    pub status: String,
    pub created_at: i64,
    pub duration_ms: i64,
    pub config: String,
    pub text: String,
    pub result: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StoredSegment {
    pub session_id: String,
    pub index: u32,
    pub path: String,
    pub rate: u32,
    pub start_sample: u64,
    pub overlap_samples: usize,
    pub reason: String,
    pub text: Option<String>,
}

fn ensure_column(conn: &Connection, table: &str, column: &str, definition: &str) -> Result<()> {
    // Names are internal constants, never caller-provided identifiers.
    let mut statement = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let names = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>>>()?;
    if !names.iter().any(|name| name == column) {
        conn.execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
            [],
        )?;
    }
    Ok(())
}

pub struct Database {
    conn: Arc<Mutex<Connection>>,
}

impl Database {
    /// Initialize database with schema
    pub fn new(db_path: PathBuf) -> Result<Self> {
        let mut connection = Connection::open(db_path)?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        let conn = connection.transaction()?;

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
            ensure_column(&conn, "transcriptions", "duration_ms", "INTEGER")?;
            ensure_column(&conn, "transcriptions", "model", "TEXT")?;
            ensure_column(&conn, "transcriptions", "cost_cents", "INTEGER")?;
            ensure_column(
                &conn,
                "transcriptions",
                "mode",
                "TEXT DEFAULT 'transcription'",
            )?;
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

        if schema_version < 3 {
            ensure_column(&conn, "pending_queue", "last_error", "TEXT")?;
            ensure_column(
                &conn,
                "pending_queue",
                "auto_retry_disabled",
                "INTEGER NOT NULL DEFAULT 0",
            )?;
            conn.execute(
                "INSERT OR REPLACE INTO settings (key, value) VALUES ('schema_version', '3')",
                [],
            )?;
            println!("📦 Database migrated to schema version 3 (queue error state)");
        }

        ensure_column(
            &conn,
            "pending_queue",
            "claimed_until",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        ensure_column(
            &conn,
            "pending_queue",
            "next_attempt_at",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        ensure_column(&conn, "transcriptions", "source_id", "TEXT")?;
        conn.execute_batch("CREATE UNIQUE INDEX IF NOT EXISTS idx_transcription_source ON transcriptions(source_id) WHERE source_id IS NOT NULL;
            CREATE TABLE IF NOT EXISTS dictation_sessions (
                id TEXT PRIMARY KEY, status TEXT NOT NULL, created_at INTEGER NOT NULL,
                duration_ms INTEGER NOT NULL DEFAULT 0, config TEXT NOT NULL, text TEXT NOT NULL DEFAULT '', result TEXT NOT NULL DEFAULT '', claimed_until INTEGER NOT NULL DEFAULT 0);
            CREATE TABLE IF NOT EXISTS dictation_segments (
                session_id TEXT NOT NULL, segment_index INTEGER NOT NULL, path TEXT NOT NULL, rate INTEGER NOT NULL,
                start_sample INTEGER NOT NULL, overlap_samples INTEGER NOT NULL, reason TEXT NOT NULL,
                text TEXT, PRIMARY KEY(session_id,segment_index));
            INSERT OR REPLACE INTO settings(key,value) VALUES ('schema_version','4');")?;
        ensure_column(&conn, "conversation_history", "source_id", "TEXT")?;
        conn.execute("CREATE UNIQUE INDEX IF NOT EXISTS idx_conversation_source ON conversation_history(source_id,role) WHERE source_id IS NOT NULL",[])?;
        conn.commit()?;

        Ok(Self {
            conn: Arc::new(Mutex::new(connection)),
        })
    }

    /// Save a new transcription to the database
    #[cfg(test)]
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
        println!(
            "💾 Saved transcription to DB (id: {}, model: {:?}, cost: {:?})",
            id, model, cost_cents
        );

        Ok(id)
    }

    /// Get total count of transcriptions
    #[cfg(test)]
    pub fn count_transcriptions(&self) -> Result<i64> {
        let conn = self.conn.lock().unwrap();

        let count: i64 =
            conn.query_row("SELECT COUNT(*) FROM transcriptions", [], |row| row.get(0))?;

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

        let result = conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
            row.get(0)
        });

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
        let mut stmt = conn
            .prepare("SELECT text FROM transcriptions WHERE timestamp >= ?1 AND timestamp <= ?2")?;
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

    #[cfg(test)]
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
            "SELECT id, mode, audio_path, prompt_text, model, created_at, retry_count, last_error, auto_retry_disabled
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
                    last_error: row.get(7)?,
                    auto_retry_disabled: row.get(8)?,
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
        let count: i64 =
            conn.query_row("SELECT COUNT(*) FROM pending_queue", [], |row| row.get(0))?;
        Ok(count)
    }

    pub fn record_queue_failure(
        &self,
        id: i64,
        last_error: &str,
        auto_retry_disabled: bool,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE pending_queue
             SET retry_count = retry_count + 1,
                 last_error = ?2,
                 auto_retry_disabled = ?3,
                 claimed_until = 0,
                 next_attempt_at = ?4 + MIN(1800000, 30000 * (1 << MIN(retry_count,6)))
             WHERE id = ?1",
            rusqlite::params![id, last_error, auto_retry_disabled, crate::now_ms()],
        )?;
        Ok(())
    }

    pub fn load_latest_transcription(&self) -> Result<Option<TranscriptionEntry>> {
        self.conn.lock().unwrap().query_row("SELECT id,text,timestamp,duration_ms,model,cost_cents,mode FROM transcriptions ORDER BY timestamp DESC,id DESC LIMIT 1",[],|row|Ok(TranscriptionEntry{id:Some(row.get(0)?),text:row.get(1)?,timestamp:row.get(2)?,duration_ms:row.get(3)?,model:row.get(4)?,cost_cents:row.get(5)?,mode:row.get(6)?})).optional()
    }
    pub fn load_history_page(&self, limit: u32, offset: u32) -> Result<Vec<TranscriptionEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt=conn.prepare("SELECT id,text,timestamp,duration_ms,model,cost_cents,mode FROM transcriptions ORDER BY timestamp DESC,id DESC LIMIT ?1 OFFSET ?2")?;
        let rows = stmt
            .query_map(rusqlite::params![limit.min(200), offset], |row| {
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
            .collect();
        rows
    }
    pub fn claim_queue_item(&self, id: i64, manual: bool) -> Result<bool> {
        let now = crate::now_ms();
        Ok(self.conn.lock().unwrap().execute("UPDATE pending_queue SET claimed_until=?2 WHERE id=?1 AND claimed_until<?3 AND (?4 OR (auto_retry_disabled=0 AND next_attempt_at<=?3))",rusqlite::params![id,now+600_000,now,manual])?==1)
    }
    pub fn create_session(&self, session: &SessionRecord) -> Result<()> {
        self.conn.lock().unwrap().execute(
            "INSERT INTO dictation_sessions(id,status,created_at,config) VALUES (?1,?2,?3,?4)",
            rusqlite::params![
                session.id,
                session.status,
                session.created_at,
                session.config
            ],
        )?;
        Ok(())
    }
    pub fn update_session(
        &self,
        id: &str,
        status: &str,
        duration_ms: i64,
        text: &str,
    ) -> Result<()> {
        self.conn.lock().unwrap().execute(
            "UPDATE dictation_sessions SET status=?2,duration_ms=?3,text=?4 WHERE id=?1",
            rusqlite::params![id, status, duration_ms, text],
        )?;
        Ok(())
    }
    pub fn recover_interrupted_sessions(&self) -> Result<()> {
        self.conn.lock().unwrap().execute("UPDATE dictation_sessions SET status='recoverable',claimed_until=0 WHERE status IN ('recording','finalizing','transcribing')",[])?;
        Ok(())
    }
    pub fn load_recoverable_sessions(&self) -> Result<Vec<SessionRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt=conn.prepare("SELECT id,status,created_at,duration_ms,config,text,result FROM dictation_sessions WHERE status IN ('recoverable','incomplete') ORDER BY created_at DESC")?;
        let rows = stmt
            .query_map([], |row| {
                Ok(SessionRecord {
                    id: row.get(0)?,
                    status: row.get(1)?,
                    created_at: row.get(2)?,
                    duration_ms: row.get(3)?,
                    config: row.get(4)?,
                    text: row.get(5)?,
                    result: row.get(6)?,
                })
            })?
            .collect();
        rows
    }
    pub fn load_session(&self, id: &str) -> Result<Option<SessionRecord>> {
        self.conn.lock().unwrap().query_row("SELECT id,status,created_at,duration_ms,config,text,result FROM dictation_sessions WHERE id=?1",[id],|row|Ok(SessionRecord{id:row.get(0)?,status:row.get(1)?,created_at:row.get(2)?,duration_ms:row.get(3)?,config:row.get(4)?,text:row.get(5)?,result:row.get(6)?})).optional()
    }
    pub fn claim_session(&self, id: &str) -> Result<bool> {
        let now = crate::now_ms();
        Ok(self.conn.lock().unwrap().execute("UPDATE dictation_sessions SET claimed_until=?2 WHERE id=?1 AND status='recoverable' AND claimed_until<?3",rusqlite::params![id,now+1_800_000,now])?==1)
    }
    pub fn release_session(&self, id: &str) -> Result<()> {
        self.conn.lock().unwrap().execute(
            "UPDATE dictation_sessions SET claimed_until=0 WHERE id=?1",
            [id],
        )?;
        Ok(())
    }
    pub fn add_segment(&self, segment: &StoredSegment) -> Result<()> {
        self.conn.lock().unwrap().execute("INSERT INTO dictation_segments(session_id,segment_index,path,rate,start_sample,overlap_samples,reason) VALUES (?1,?2,?3,?4,?5,?6,?7)",rusqlite::params![segment.session_id,segment.index,segment.path,segment.rate,segment.start_sample,segment.overlap_samples as u64,segment.reason])?;
        Ok(())
    }
    pub fn complete_segment(&self, id: &str, index: u32, text: &str) -> Result<()> {
        self.conn.lock().unwrap().execute(
            "UPDATE dictation_segments SET text=?3 WHERE session_id=?1 AND segment_index=?2",
            rusqlite::params![id, index, text],
        )?;
        Ok(())
    }
    pub fn load_segments(&self, id: &str) -> Result<Vec<StoredSegment>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt=conn.prepare("SELECT session_id,segment_index,path,rate,start_sample,overlap_samples,reason,text FROM dictation_segments WHERE session_id=?1 ORDER BY segment_index")?;
        let rows = stmt
            .query_map([id], |row| {
                Ok(StoredSegment {
                    session_id: row.get(0)?,
                    index: row.get(1)?,
                    path: row.get(2)?,
                    rate: row.get(3)?,
                    start_sample: row.get(4)?,
                    overlap_samples: row.get::<_, u64>(5)? as usize,
                    reason: row.get(6)?,
                    text: row.get(7)?,
                })
            })?
            .collect();
        rows
    }
    pub fn save_result_once(
        &self,
        source: &str,
        text: &str,
        duration: Option<i64>,
        model: &str,
        mode: &str,
    ) -> Result<()> {
        let cost = crate::estimate_cost_cents(model, duration, text);
        self.conn.lock().unwrap().execute("INSERT OR IGNORE INTO transcriptions(text,timestamp,duration_ms,model,mode,source_id,cost_cents) VALUES (?1,?2,?3,?4,?5,?6,?7)",rusqlite::params![text,crate::now_ms(),duration,model,mode,source,cost])?;
        Ok(())
    }
    pub fn set_session_config(&self, id: &str, config: &str) -> Result<()> {
        self.conn.lock().unwrap().execute(
            "UPDATE dictation_sessions SET config=?2 WHERE id=?1",
            rusqlite::params![id, config],
        )?;
        Ok(())
    }
    pub fn stage_session_result(&self, id: &str, result: &str) -> Result<()> {
        self.conn.lock().unwrap().execute(
            "UPDATE dictation_sessions SET result=?2 WHERE id=?1",
            rusqlite::params![id, result],
        )?;
        Ok(())
    }
    pub fn load_result_for_source(&self, source: &str) -> Result<Option<String>> {
        self.conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT text FROM transcriptions WHERE source_id=?1",
                [source],
                |row| row.get(0),
            )
            .optional()
    }
    pub fn append_conversation_once(&self, source: &str, prompt: &str, result: &str) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let now = crate::now_ms();
        tx.execute("INSERT OR IGNORE INTO conversation_history(role,content,timestamp,source_id) VALUES ('user',?1,?2,?3)",rusqlite::params![prompt,now-1,source])?;
        tx.execute("INSERT OR IGNORE INTO conversation_history(role,content,timestamp,source_id) VALUES ('assistant',?1,?2,?3)",rusqlite::params![result,now,source])?;
        tx.commit()
    }
}
