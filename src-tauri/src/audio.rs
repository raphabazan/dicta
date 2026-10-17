use cpal::traits::{DeviceTrait, HostTrait};
pub fn get_input_device_by_name(name: Option<&str>) -> Result<cpal::Device, String> {
    let host = cpal::default_host();
    if let Some(name) = name {
        let requested = name.trim();
        for device in host
            .input_devices()
            .map_err(|_| "Não foi possível listar microfones.".to_string())?
        {
            if device
                .name()
                .map(|n| n.trim() == requested)
                .unwrap_or(false)
            {
                return Ok(device);
            }
        }
        crate::diagnostics::event(
            "microphone_fallback",
            serde_json::json!({"reason":"selected_device_missing"}),
        );
    }
    host.default_input_device()
        .ok_or_else(|| "Nenhum microfone disponível.".into())
}
