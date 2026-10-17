//! A single clipboard transaction for every delivery source.
use serde::{Deserialize, Serialize};
use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager};

static SERIAL: Mutex<()> = Mutex::new(());
static EPOCH: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Target {
    pub window: isize,
    pub process: u32,
    pub epoch: u64,
}
#[derive(Clone, Debug, Serialize)]
pub struct Outcome {
    pub status: &'static str,
    pub code: String,
}

pub fn invalidate() {
    EPOCH.fetch_add(1, Ordering::SeqCst);
}
pub fn capture_target() -> Option<Target> {
    native::foreground()
}

trait Desktop {
    type Snapshot;
    fn target_valid(&self, target: Target) -> bool;
    fn modifiers_free(&self) -> bool;
    fn snapshot(&mut self) -> Result<Self::Snapshot, String>;
    fn write_text(&mut self, text: &str) -> Result<u32, String>;
    fn failed_write_sequence(&self) -> Option<u32> {
        None
    }
    fn owns_clipboard(&self, _: u32) -> bool {
        true
    }
    fn inject(&mut self) -> Result<(), String>;
    fn restore(&mut self, original: Self::Snapshot, owned_sequence: u32) -> Result<(), String>;
    fn wait(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

fn transaction<D: Desktop>(desktop: &mut D, text: &str, target: Target) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err("empty_text".into());
    }
    if !desktop.target_valid(target) {
        return Err("destination_changed".into());
    }
    let start = Instant::now();
    while !desktop.modifiers_free() {
        if start.elapsed() >= Duration::from_millis(1500) {
            return Err("modifiers_held".into());
        }
        desktop.wait(Duration::from_millis(20));
    }
    let original = desktop.snapshot()?; // Never overwrite something we cannot restore.
    if !desktop.target_valid(target) {
        return Err("destination_changed".into());
    }
    let owned = match desktop.write_text(text) {
        Ok(sequence) => sequence,
        Err(error) => {
            if let Some(sequence) = desktop.failed_write_sequence() {
                let _ = desktop.restore(original, sequence);
            }
            return Err(error);
        }
    };
    // Native injection and destination confirmation are different things.
    let injected = if !desktop.owns_clipboard(owned) {
        Err("clipboard_changed".into())
    } else if desktop.target_valid(target) && desktop.modifiers_free() {
        desktop.inject()
    } else {
        Err("destination_changed".into())
    };
    if injected.is_ok() {
        desktop.wait(Duration::from_millis(750));
    }
    let restored = desktop.restore(original, owned);
    injected?;
    restored?;
    Ok(())
}

pub fn deliver(app: &AppHandle, text: &str, target: Option<Target>) -> Outcome {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let start = Instant::now();
    let result = target
        .ok_or_else(|| "destination_unavailable".to_string())
        .and_then(|target| {
            let owner = app
                .get_webview_window("main")
                .and_then(|w| w.hwnd().ok())
                .map(|h| h.0 as isize)
                .unwrap_or(0);
            transaction(&mut native::WindowsDesktop::new(owner), text, target)
        });
    let outcome = match result {
        Ok(()) => Outcome {
            status: "sent",
            code: "shortcut_sent".into(),
        },
        Err(code) => Outcome {
            status: "recoverable",
            code,
        },
    };
    crate::diagnostics::event(
        "delivery",
        serde_json::json!({"status":outcome.status,"code":outcome.code,"target":target,"foreground":capture_target(),"elapsed_ms":start.elapsed().as_millis()}),
    );
    let _ = app.emit("delivery-result", &outcome);
    outcome
}

#[cfg(windows)]
mod native {
    use super::*;
    use windows::Win32::{
        Foundation::{
            GetLastError, GlobalFree, SetLastError, ERROR_SUCCESS, HANDLE, HGLOBAL, HWND,
        },
        System::{DataExchange::*, Memory::*},
        UI::{
            Input::KeyboardAndMouse::*,
            WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId},
        },
    };
    pub fn foreground() -> Option<Target> {
        unsafe {
            let hwnd = GetForegroundWindow();
            let mut process = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut process));
            if hwnd.0.is_null() || process == 0 || process == std::process::id() {
                return None;
            }
            Some(Target {
                window: hwnd.0 as isize,
                process,
                epoch: EPOCH.load(Ordering::SeqCst),
            })
        }
    }
    struct ClipboardLock;
    impl Drop for ClipboardLock {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseClipboard();
            }
        }
    }
    fn lock(owner: isize) -> Result<ClipboardLock, String> {
        let start = Instant::now();
        loop {
            if unsafe { OpenClipboard(HWND(owner as *mut _)) }.is_ok() {
                return Ok(ClipboardLock);
            }
            if start.elapsed() > Duration::from_millis(800) {
                return Err("clipboard_busy".into());
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }
    struct Allocation(HGLOBAL);
    impl Allocation {
        fn bytes(data: &[u8]) -> Result<Self, String> {
            unsafe {
                let memory = GlobalAlloc(GMEM_MOVEABLE, data.len().max(1))
                    .map_err(|_| "clipboard_allocation".to_string())?;
                let value = Self(memory);
                let pointer = GlobalLock(memory);
                if pointer.is_null() {
                    return Err("clipboard_allocation".into());
                }
                std::ptr::copy_nonoverlapping(data.as_ptr(), pointer as *mut u8, data.len());
                let _ = GlobalUnlock(memory);
                Ok(value)
            }
        }
        unsafe fn transfer(self, format: u32) -> Result<(), String> {
            SetClipboardData(format, HANDLE(self.0 .0))
                .map_err(|_| "clipboard_write".to_string())?;
            std::mem::forget(self);
            Ok(())
        }
    }
    impl Drop for Allocation {
        fn drop(&mut self) {
            unsafe {
                let _ = GlobalFree(self.0);
            }
        }
    }
    pub struct WindowsDesktop {
        owner: isize,
        snapshot_sequence: u32,
        failed_write: Option<u32>,
    }
    impl WindowsDesktop {
        pub fn new(owner: isize) -> Self {
            Self {
                owner,
                snapshot_sequence: 0,
                failed_write: None,
            }
        }
    }
    impl Desktop for WindowsDesktop {
        type Snapshot = Vec<(u32, Vec<u8>)>;
        fn target_valid(&self, target: Target) -> bool {
            foreground() == Some(target)
        }
        fn modifiers_free(&self) -> bool {
            unsafe {
                [0x10, 0x11, 0x12, 0x5b, 0x5c]
                    .iter()
                    .all(|key| GetAsyncKeyState(*key) >= 0)
            }
        }
        fn snapshot(&mut self) -> Result<Self::Snapshot, String> {
            unsafe {
                let _guard = lock(self.owner)?;
                let mut formats = Vec::new();
                let mut current = 0;
                loop {
                    SetLastError(ERROR_SUCCESS);
                    current = EnumClipboardFormats(current);
                    if current == 0 {
                        if GetLastError() != ERROR_SUCCESS {
                            return Err("clipboard_snapshot".into());
                        }
                        break;
                    }
                    formats.push(current);
                }
                let dib = formats.contains(&8) || formats.contains(&17);
                let mut result = Vec::new();
                let mut total = 0usize;
                for format in formats {
                    // DIB carries the image; Windows regenerates a bitmap if requested.
                    if format == 2 && dib {
                        continue;
                    }
                    if matches!(format, 2 | 3 | 9 | 14) || (0x200..0x300).contains(&format) {
                        return Err("clipboard_format_not_restorable".into());
                    }
                    let handle =
                        GetClipboardData(format).map_err(|_| "clipboard_snapshot".to_string())?;
                    let memory = HGLOBAL(handle.0);
                    let size = GlobalSize(memory);
                    total = total.saturating_add(size);
                    if size == 0 || total > 64 * 1024 * 1024 {
                        return Err("clipboard_format_not_restorable".into());
                    }
                    let pointer = GlobalLock(memory);
                    if pointer.is_null() {
                        return Err("clipboard_snapshot".into());
                    }
                    let bytes = std::slice::from_raw_parts(pointer as *const u8, size).to_vec();
                    let _ = GlobalUnlock(memory);
                    result.push((format, bytes));
                }
                self.snapshot_sequence = GetClipboardSequenceNumber();
                Ok(result)
            }
        }
        fn write_text(&mut self, text: &str) -> Result<u32, String> {
            unsafe {
                let bytes: Vec<u8> = text
                    .encode_utf16()
                    .chain(Some(0))
                    .flat_map(u16::to_le_bytes)
                    .collect();
                let allocated = Allocation::bytes(&bytes)?;
                let _guard = lock(self.owner)?;
                if GetClipboardSequenceNumber() != self.snapshot_sequence {
                    return Err("clipboard_changed".into());
                }
                EmptyClipboard().map_err(|_| "clipboard_write".to_string())?;
                if let Err(error) = allocated.transfer(13) {
                    self.failed_write = Some(GetClipboardSequenceNumber());
                    return Err(error);
                }
                drop(_guard);
                if GetClipboardOwner().ok().map(|h| h.0 as isize) != Some(self.owner) {
                    return Err("clipboard_changed".into());
                }
                Ok(GetClipboardSequenceNumber())
            }
        }
        fn failed_write_sequence(&self) -> Option<u32> {
            self.failed_write
        }
        fn owns_clipboard(&self, sequence: u32) -> bool {
            unsafe {
                GetClipboardSequenceNumber() == sequence
                    && GetClipboardOwner().ok().map(|h| h.0 as isize) == Some(self.owner)
            }
        }
        fn inject(&mut self) -> Result<(), String> {
            unsafe {
                let input = |key: u16, up: bool| INPUT {
                    r#type: INPUT_KEYBOARD,
                    Anonymous: INPUT_0 {
                        ki: KEYBDINPUT {
                            wVk: VIRTUAL_KEY(key),
                            dwFlags: if up {
                                KEYEVENTF_KEYUP
                            } else {
                                KEYBD_EVENT_FLAGS(0)
                            },
                            ..Default::default()
                        },
                    },
                };
                let sequence = [
                    input(0x11, false),
                    input(0x56, false),
                    input(0x56, true),
                    input(0x11, true),
                ];
                let sent = SendInput(&sequence, std::mem::size_of::<INPUT>() as i32);
                if sent != sequence.len() as u32 {
                    if sent > 0 {
                        let cleanup = [input(0x56, true), input(0x11, true)];
                        SendInput(&cleanup, std::mem::size_of::<INPUT>() as i32);
                    }
                    return Err("keyboard_injection_failed".into());
                }
                Ok(())
            }
        }
        fn restore(&mut self, original: Self::Snapshot, owned: u32) -> Result<(), String> {
            unsafe {
                // Allocate before EmptyClipboard so allocation failure cannot erase data.
                let allocated: Result<Vec<_>, _> = original
                    .into_iter()
                    .map(|(f, b)| Allocation::bytes(&b).map(|a| (f, a)))
                    .collect();
                let allocated = allocated?;
                let _guard = lock(self.owner)?;
                if GetClipboardSequenceNumber() != owned {
                    return Ok(());
                }
                EmptyClipboard().map_err(|_| "clipboard_restore".to_string())?;
                for (format, memory) in allocated {
                    memory.transfer(format)?;
                }
                Ok(())
            }
        }
    }
}

#[cfg(not(windows))]
mod native {
    use super::*;
    pub fn foreground() -> Option<Target> {
        None
    }
    pub struct WindowsDesktop;
    impl WindowsDesktop {
        pub fn new(_: isize) -> Self {
            Self
        }
    }
    impl Desktop for WindowsDesktop {
        type Snapshot = ();
        fn target_valid(&self, _: Target) -> bool {
            false
        }
        fn modifiers_free(&self) -> bool {
            false
        }
        fn snapshot(&mut self) -> Result<(), String> {
            Err("unsupported_platform".into())
        }
        fn write_text(&mut self, _: &str) -> Result<u32, String> {
            Err("unsupported_platform".into())
        }
        fn inject(&mut self) -> Result<(), String> {
            Err("unsupported_platform".into())
        }
        fn restore(&mut self, _: (), _: u32) -> Result<(), String> {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Fake {
        valid: bool,
        clipboard: String,
        sequence: u32,
        writes: usize,
        unsupported: bool,
        injection_error: bool,
        copy_during_paste: bool,
        write_failure: bool,
        stolen_before_injection: bool,
        injections: usize,
    }
    impl Desktop for Fake {
        type Snapshot = String;
        fn target_valid(&self, _: Target) -> bool {
            self.valid
        }
        fn modifiers_free(&self) -> bool {
            true
        }
        fn snapshot(&mut self) -> Result<String, String> {
            if self.unsupported {
                Err("clipboard_format_not_restorable".into())
            } else {
                Ok(self.clipboard.clone())
            }
        }
        fn write_text(&mut self, text: &str) -> Result<u32, String> {
            self.clipboard = text.into();
            self.writes += 1;
            self.sequence += 1;
            if self.write_failure {
                return Err("clipboard_write".into());
            }
            let owned = self.sequence;
            if self.stolen_before_injection {
                self.clipboard = "new user copy".into();
                self.sequence += 1;
            }
            Ok(owned)
        }
        fn failed_write_sequence(&self) -> Option<u32> {
            self.write_failure.then_some(self.sequence)
        }
        fn owns_clipboard(&self, sequence: u32) -> bool {
            self.sequence == sequence
        }
        fn inject(&mut self) -> Result<(), String> {
            self.injections += 1;
            if self.copy_during_paste {
                self.clipboard = "new user copy".into();
                self.sequence += 1;
            }
            if self.injection_error {
                Err("keyboard_injection_failed".into())
            } else {
                Ok(())
            }
        }
        fn restore(&mut self, old: String, seq: u32) -> Result<(), String> {
            if self.sequence == seq {
                self.clipboard = old;
            }
            Ok(())
        }
        fn wait(&self, _: Duration) {}
    }
    fn target() -> Target {
        Target {
            window: 1,
            process: 2,
            epoch: 1,
        }
    }
    #[test]
    fn unsupported_clipboard_is_not_overwritten() {
        let mut d = Fake {
            valid: true,
            unsupported: true,
            ..Default::default()
        };
        assert!(transaction(&mut d, "new", target()).is_err());
        assert_eq!(d.writes, 0);
    }
    #[test]
    fn restores_snapshot_even_when_keyboard_fails() {
        let mut d = Fake {
            valid: true,
            clipboard: "image+html+text".into(),
            injection_error: true,
            ..Default::default()
        };
        assert!(transaction(&mut d, "new", target()).is_err());
        assert_eq!(d.clipboard, "image+html+text");
    }
    #[test]
    fn new_user_copy_survives() {
        let mut d = Fake {
            valid: true,
            copy_during_paste: true,
            ..Default::default()
        };
        transaction(&mut d, "new", target()).unwrap();
        assert_eq!(d.clipboard, "new user copy");
    }
    #[test]
    fn changed_target_never_touches_clipboard() {
        let mut d = Fake::default();
        assert!(transaction(&mut d, "new", target()).is_err());
        assert_eq!(d.writes, 0);
    }
    #[test]
    fn success_only_means_shortcut_sent() {
        let mut d = Fake {
            valid: true,
            clipboard: "original".into(),
            ..Default::default()
        };
        transaction(&mut d, "new", target()).unwrap();
        assert_eq!(d.clipboard, "original");
    }
    #[test]
    fn failed_write_rolls_back_original_before_returning() {
        let mut d = Fake {
            valid: true,
            clipboard: "original".into(),
            write_failure: true,
            ..Default::default()
        };
        assert_eq!(
            transaction(&mut d, "new", target()).unwrap_err(),
            "clipboard_write"
        );
        assert_eq!(d.clipboard, "original");
        assert_eq!(d.injections, 0);
    }
    #[test]
    fn clipboard_changed_before_injection_never_pastes_other_content() {
        let mut d = Fake {
            valid: true,
            stolen_before_injection: true,
            ..Default::default()
        };
        assert_eq!(
            transaction(&mut d, "new", target()).unwrap_err(),
            "clipboard_changed"
        );
        assert_eq!(d.clipboard, "new user copy");
        assert_eq!(d.injections, 0);
    }
}
