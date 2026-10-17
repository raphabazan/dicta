//! Small local diagnostic log. Callers pass metadata, never audio/text/credentials.
use serde_json::{json, Value};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::{Mutex, OnceLock},
};

static DIRECTORY: OnceLock<PathBuf> = OnceLock::new();
static WRITER: Mutex<()> = Mutex::new(());

pub fn init(directory: PathBuf) {
    if fs::create_dir_all(&directory).is_ok() {
        let _ = DIRECTORY.set(directory);
    }
}

pub fn event(kind: &str, metadata: Value) {
    let Some(directory) = DIRECTORY.get() else {
        return;
    };
    let Ok(_guard) = WRITER.lock() else { return };
    let file = directory.join("runtime.jsonl");
    if fs::metadata(&file)
        .map(|m| m.len() > 2_000_000)
        .unwrap_or(false)
    {
        let _ = fs::remove_file(directory.join("runtime.2.jsonl"));
        let _ = fs::rename(
            directory.join("runtime.1.jsonl"),
            directory.join("runtime.2.jsonl"),
        );
        let _ = fs::rename(&file, directory.join("runtime.1.jsonl"));
    }
    if let Ok(mut output) = OpenOptions::new().create(true).append(true).open(file) {
        let _ = writeln!(
            output,
            "{}",
            json!({"at": crate::now_ms(), "event": kind, "build": build_id(), "metadata": metadata})
        );
    }
}

pub fn build_id() -> &'static str {
    env!("DICTA_BUILD_ID")
}
