//! Reconcile suspended sessions; waking the PC never initiates a delivery.
use tauri::{AppHandle, Emitter, Manager};

#[cfg(windows)]
pub struct Instance(windows::Win32::Foundation::HANDLE);
#[cfg(windows)]
impl Instance {
    pub fn acquire(profile: &std::path::Path) -> Result<Self, String> {
        use std::hash::{Hash, Hasher};
        use windows::{
            core::PCWSTR,
            Win32::{
                Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS},
                System::Threading::CreateMutexW,
            },
        };
        let profile = profile
            .canonicalize()
            .map_err(|_| "profile_unavailable")?
            .to_string_lossy()
            .to_lowercase();
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        profile.hash(&mut hash);
        let name: Vec<u16> = format!("Local\\Dicta-{:x}", hash.finish())
            .encode_utf16()
            .chain(Some(0))
            .collect();
        unsafe {
            let handle = CreateMutexW(None, false, PCWSTR(name.as_ptr()))
                .map_err(|_| "profile_lock_failed")?;
            if GetLastError() == ERROR_ALREADY_EXISTS {
                let _ = CloseHandle(handle);
                return Err("profile_already_open".into());
            }
            Ok(Self(handle))
        }
    }
}
#[cfg(windows)]
impl Drop for Instance {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.0);
        }
    }
}
#[cfg(not(windows))]
pub struct Instance;
#[cfg(not(windows))]
impl Instance {
    pub fn acquire(_: &std::path::Path) -> Result<Self, String> {
        Ok(Self)
    }
}

fn interrupted(app: &AppHandle) {
    crate::delivery::invalidate();
    if let Some(state) = app.try_state::<crate::AppState>() {
        state.sessions.interrupt(app, false);
        *state.is_recording.lock().unwrap_or_else(|e| e.into_inner()) = false;
        *state.tts_active.lock().unwrap_or_else(|e| e.into_inner()) = false;
        if let Some(sink) = state
            .tts_sink
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            sink.stop();
        }
    }
    let _ = crate::system_audio::unmute_system_audio();
}

#[cfg(windows)]
mod windows_events {
    use super::*;
    use windows::Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        System::RemoteDesktop::{WTSRegisterSessionNotification, WTSUnRegisterSessionNotification},
        UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
    };
    const ID: usize = 0x44494354;
    unsafe extern "system" fn callback(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _id: usize,
        data: usize,
    ) -> LRESULT {
        let app = &*(data as *const AppHandle);
        let _ =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match (message, wparam.0) {
                (0x218, 4) | (0x2b1, 7) | (0x2b1, 6) | (0x2b1, 2) | (0x2b1, 4) => {
                    crate::diagnostics::event(
                        "session_suspended",
                        serde_json::json!({"message":message,"kind":wparam.0}),
                    );
                    interrupted(app);
                }
                (0x218, 18) | (0x218, 7) | (0x2b1, 8) => {
                    crate::delivery::invalidate();
                    crate::diagnostics::event(
                        "session_resumed",
                        serde_json::json!({"message":message,"kind":wparam.0}),
                    );
                    let _ = app.emit("runtime-resumed", ());
                }
                _ => {}
            }));
        if message == 0x82 {
            let _ = WTSUnRegisterSessionNotification(hwnd);
            let _ = RemoveWindowSubclass(hwnd, Some(callback), ID);
            drop(Box::from_raw(data as *mut AppHandle));
        }
        DefSubclassProc(hwnd, message, wparam, lparam)
    }
    pub fn install(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
        let window = app
            .get_webview_window("main")
            .ok_or("Missing main window")?;
        let hwnd = HWND(window.hwnd()?.0 as *mut _);
        let pointer = Box::into_raw(Box::new(app.clone()));
        if !unsafe { SetWindowSubclass(hwnd, Some(callback), ID, pointer as usize) }.as_bool() {
            unsafe {
                drop(Box::from_raw(pointer));
            }
            return Err("Could not install Windows lifecycle hook".into());
        }
        if unsafe { WTSRegisterSessionNotification(hwnd, 0) }.is_err() {
            crate::diagnostics::event("session_notifications_unavailable", serde_json::json!({}));
        }
        Ok(())
    }
}
#[cfg(windows)]
pub use windows_events::install;
#[cfg(not(windows))]
pub fn install(_: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}
