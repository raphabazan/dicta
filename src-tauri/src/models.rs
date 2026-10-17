pub const DEFAULT_COMMAND: &str = "gpt-6.1-sol";
pub const LIGHT_COMMAND: &str = "gpt-6-luna";

pub fn normalize(model: &str) -> &str {
    match model {
        "gpt-5.4" | "gpt-4o" => DEFAULT_COMMAND,
        "gpt-4o-mini" => LIGHT_COMMAND,
        "gpt-6.1-sol" | "gpt-6-luna" => model,
        _ => "transcribe-only",
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn migrates_each_command_tier_without_changing_dictation() {
        assert_eq!(super::normalize("gpt-5.4"), "gpt-6.1-sol");
        assert_eq!(super::normalize("gpt-4o-mini"), "gpt-6-luna");
        assert_eq!(super::normalize("transcribe-only"), "transcribe-only");
        assert_eq!(super::normalize("unknown"), "transcribe-only");
    }
}
