//! Shared model override validation.

const MAX_MODEL_CHARS: usize = 128;
const VALID_EFFORTS: &[&str] = &["low", "medium", "high", "xhigh"];

/// Normalize an optional user-supplied model override.
pub fn normalize_model(value: Option<String>) -> anyhow::Result<Option<String>> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.chars().any(|ch| ch.is_ascii_control()) {
        anyhow::bail!("model override must not contain control characters");
    }
    let model = value.trim();
    if model.is_empty() {
        return Ok(None);
    }
    if model.chars().count() > MAX_MODEL_CHARS {
        anyhow::bail!("model override exceeds {MAX_MODEL_CHARS} characters");
    }
    Ok(Some(model.to_string()))
}

/// Render a model override as a Codex config argument.
pub fn model_config_arg(model: &str) -> String {
    format!("model=\"{}\"", escape_toml_string(model))
}

/// Normalize an optional model reasoning effort override.
pub fn normalize_reasoning_effort(value: Option<String>) -> anyhow::Result<Option<String>> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.chars().any(|ch| ch.is_ascii_control()) {
        anyhow::bail!("reasoning effort must not contain control characters");
    }
    let effort = value.trim();
    if effort.is_empty() || effort == "default" {
        return Ok(None);
    }
    if !VALID_EFFORTS.contains(&effort) {
        anyhow::bail!(
            "reasoning effort must be one of: {}",
            VALID_EFFORTS.join(", ")
        );
    }
    Ok(Some(effort.to_string()))
}

/// Render a reasoning effort override as a Codex config argument.
pub fn reasoning_effort_config_arg(effort: &str) -> String {
    format!("model_reasoning_effort=\"{effort}\"")
}

fn escape_toml_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::{
        model_config_arg, normalize_model, normalize_reasoning_effort, reasoning_effort_config_arg,
    };

    #[test]
    fn normalizes_optional_models() {
        assert_eq!(normalize_model(None).unwrap(), None);
        assert_eq!(normalize_model(Some("  ".to_string())).unwrap(), None);
        assert_eq!(
            normalize_model(Some(" gpt-5.4-mini ".to_string())).unwrap(),
            Some("gpt-5.4-mini".to_string())
        );
    }

    #[test]
    fn rejects_invalid_models() {
        assert!(normalize_model(Some("x\n".to_string())).is_err());
        assert!(normalize_model(Some("x".repeat(129))).is_err());
    }

    #[test]
    fn renders_codex_config_string() {
        assert_eq!(model_config_arg("gpt-5.4-mini"), "model=\"gpt-5.4-mini\"");
        assert_eq!(model_config_arg("a\"b"), "model=\"a\\\"b\"");
    }

    #[test]
    fn normalizes_reasoning_efforts() {
        assert_eq!(normalize_reasoning_effort(None).unwrap(), None);
        assert_eq!(
            normalize_reasoning_effort(Some("default".to_string())).unwrap(),
            None
        );
        assert_eq!(
            normalize_reasoning_effort(Some(" low ".to_string())).unwrap(),
            Some("low".to_string())
        );
        assert!(normalize_reasoning_effort(Some("fast".to_string())).is_err());
        assert_eq!(
            reasoning_effort_config_arg("high"),
            "model_reasoning_effort=\"high\""
        );
    }
}
