//! Native update supervision, including when the webview is hidden.
use serde::Serialize;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager as _, State};
use tauri_plugin_updater::UpdaterExt;
use tokio::sync::{Notify, OwnedRwLockReadGuard, RwLock};

#[derive(Clone, Default, Serialize)]
pub struct Status {
    pub sequence: u64,
    pub phase: String,
    pub version: String,
}

#[derive(Default)]
pub struct Manager {
    activity: Arc<RwLock<()>>,
    wake: Notify,
    status: Mutex<Status>,
}

impl Manager {
    pub fn activity(&self) -> Result<OwnedRwLockReadGuard<()>, String> {
        self.activity.clone().try_read_owned().map_err(|_| {
            "O Dicta está instalando uma atualização. Aguarde a reabertura do aplicativo.".into()
        })
    }

    pub fn request_check(&self) {
        self.wake.notify_one();
    }

    async fn wait_for_check(&self, interval: Duration, resume_delay: Duration) {
        tokio::select! {
            _ = tokio::time::sleep(interval) => {},
            _ = self.wake.notified() => {
                // Windows may report resume before networking is available.
                tokio::time::sleep(resume_delay).await;
            },
        }
    }

    fn publish(&self, app: &AppHandle, phase: &str, version: &str) {
        let status = {
            let mut current = self.status.lock().unwrap();
            *current = Status {
                sequence: current.sequence + 1,
                phase: phase.into(),
                version: version.into(),
            };
            current.clone()
        };
        crate::diagnostics::event(
            "update_state",
            serde_json::json!({"phase":phase,"version":version}),
        );
        let _ = app.emit("update-state", status);
    }
}

#[tauri::command]
pub fn get_update_status(state: State<'_, crate::AppState>) -> Status {
    state.updates.status.lock().unwrap().clone()
}

#[tauri::command]
pub fn check_updates(state: State<'_, crate::AppState>) {
    state.updates.request_check();
}

async fn check_and_install(app: &AppHandle, manager: &Manager) -> Result<(), String> {
    manager.publish(app, "checking", "");
    let updater = app
        .updater_builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| e.to_string())?;
    let Some(update) = updater.check().await.map_err(|e| e.to_string())? else {
        manager.publish(app, "current", "");
        return Ok(());
    };
    manager.publish(app, "downloading", &update.version);
    // The plugin verifies the release signature before returning these bytes.
    let bytes = update
        .download(|_, _| {}, || {})
        .await
        .map_err(|e| e.to_string())?;
    manager.publish(app, "waiting", &update.version);
    // Writer priority prevents a new hotkey/task from racing the installation.
    // Readers cover capture, processing, persistence and the complete paste attempt.
    let _exclusive = manager.activity.clone().write_owned().await;
    manager.publish(app, "installing", &update.version);
    tokio::time::sleep(Duration::from_secs(2)).await;
    update.install(bytes).map_err(|e| e.to_string())?;
    app.restart();
}

pub fn start(app: AppHandle) {
    // Development/test profiles must never install a production release.
    if cfg!(debug_assertions) {
        return;
    }
    tauri::async_runtime::spawn(async move {
        let state = app.state::<crate::AppState>();
        let manager = &state.updates;
        loop {
            let delay = match check_and_install(&app, manager).await {
                Ok(()) => Duration::from_secs(15 * 60),
                Err(error) => {
                    crate::diagnostics::event("update_failed", serde_json::json!({"error":error}));
                    manager.publish(&app, "error", "");
                    Duration::from_secs(60)
                }
            };
            manager.wait_for_check(delay, Duration::from_secs(10)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn periodic_check_runs_without_a_window_or_resume_event() {
        let manager = Manager::default();
        tokio::time::timeout(
            Duration::from_secs(1),
            manager.wait_for_check(Duration::from_millis(10), Duration::ZERO),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn resume_shortens_the_periodic_wait() {
        let manager = Manager::default();
        let waiting = manager.wait_for_check(Duration::from_secs(900), Duration::ZERO);
        manager.request_check();
        tokio::time::timeout(Duration::from_secs(1), waiting)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn install_waits_for_all_work_and_blocks_new_recordings() {
        let manager = Manager::default();
        let recording = manager.activity().unwrap();
        let queue = manager.activity().unwrap();
        let paste = manager.activity().unwrap();
        let mut install = Box::pin(manager.activity.clone().write_owned());
        assert!(tokio::time::timeout(Duration::from_millis(5), &mut install)
            .await
            .is_err());
        assert!(manager.activity().is_err());
        drop(recording);
        drop(queue);
        assert!(tokio::time::timeout(Duration::from_millis(5), &mut install)
            .await
            .is_err());
        drop(paste);
        let exclusive = tokio::time::timeout(Duration::from_secs(1), install)
            .await
            .unwrap();
        assert!(manager.activity().is_err());
        drop(exclusive);
        assert!(manager.activity().is_ok());
    }

    #[tokio::test]
    async fn failed_or_cancelled_install_releases_work_gate() {
        let manager = Manager::default();
        let work = manager.activity().unwrap();
        let mut install = Box::pin(manager.activity.clone().write_owned());
        assert!(tokio::time::timeout(Duration::from_millis(5), &mut install)
            .await
            .is_err());
        drop(install);
        assert!(manager.activity().is_ok());
        drop(work);
        let exclusive = manager.activity.clone().write_owned().await;
        drop(exclusive);
        assert!(manager.activity().is_ok());
    }

    #[tokio::test]
    async fn resume_requests_survive_until_supervisor_can_check() {
        let manager = Manager::default();
        manager.request_check();
        manager.request_check();
        tokio::time::timeout(Duration::from_secs(1), manager.wake.notified())
            .await
            .unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(5), manager.wake.notified())
                .await
                .is_err()
        );
        manager.request_check();
        tokio::time::timeout(Duration::from_secs(1), manager.wake.notified())
            .await
            .unwrap();
    }
}
