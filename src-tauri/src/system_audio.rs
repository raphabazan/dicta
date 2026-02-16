use std::sync::Mutex;
use windows::Win32::Media::Audio::*;
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::System::Com::*;

static WAS_MUTED_BEFORE: Mutex<Option<bool>> = Mutex::new(None);

/// Mute system audio output. Saves current mute state first so we can restore it later.
pub fn mute_system_audio() -> Result<(), String> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED).ok();

        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|e| format!("CoCreateInstance failed: {}", e))?;

        let device = enumerator
            .GetDefaultAudioEndpoint(eRender, eMultimedia)
            .map_err(|e| format!("GetDefaultAudioEndpoint failed: {}", e))?;

        let volume: IAudioEndpointVolume = device
            .Activate(CLSCTX_ALL, None)
            .map_err(|e| format!("Activate IAudioEndpointVolume failed: {}", e))?;

        let current_mute = volume
            .GetMute()
            .map_err(|e| format!("GetMute failed: {}", e))?;

        *WAS_MUTED_BEFORE.lock().unwrap() = Some(current_mute.as_bool());

        volume
            .SetMute(true, std::ptr::null())
            .map_err(|e| format!("SetMute(true) failed: {}", e))?;

        println!("🔇 System audio muted (was_muted_before={})", current_mute.as_bool());
        Ok(())
    }
}

/// Restore system audio to its state before we muted it.
/// If the user already had it muted, we leave it muted.
pub fn unmute_system_audio() -> Result<(), String> {
    let was_muted = {
        let guard = WAS_MUTED_BEFORE.lock().unwrap();
        guard.clone()
    };

    match was_muted {
        Some(true) => {
            // User already had system muted before recording, leave it muted
            *WAS_MUTED_BEFORE.lock().unwrap() = None;
            println!("🔇 System was already muted before recording, leaving muted");
            Ok(())
        }
        Some(false) => {
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED).ok();

                let enumerator: IMMDeviceEnumerator =
                    CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                        .map_err(|e| format!("CoCreateInstance failed: {}", e))?;

                let device = enumerator
                    .GetDefaultAudioEndpoint(eRender, eMultimedia)
                    .map_err(|e| format!("GetDefaultAudioEndpoint failed: {}", e))?;

                let volume: IAudioEndpointVolume = device
                    .Activate(CLSCTX_ALL, None)
                    .map_err(|e| format!("Activate IAudioEndpointVolume failed: {}", e))?;

                volume
                    .SetMute(false, std::ptr::null())
                    .map_err(|e| format!("SetMute(false) failed: {}", e))?;

                // Only clear state after successful unmute
                *WAS_MUTED_BEFORE.lock().unwrap() = None;

                println!("🔊 System audio unmuted (restored to pre-recording state)");
                Ok(())
            }
        }
        None => {
            // No mute operation was recorded, nothing to restore
            Ok(())
        }
    }
}
