use serde_json::Value;

const SECRET_HINTS: [&str; 7] = ["token", "access", "refresh", "authorization", "cookie", "secret", "password"];
const MAX_CHARS: usize = 2000;

/// Masks values under secret-looking keys in JSON lines and truncates the text.
/// Applied to every CLI output before it reaches the log file.
pub fn redact(text: &str) -> String {
    let joined = text
        .lines()
        .map(|line| match serde_json::from_str::<Value>(line) {
            Ok(mut value) if value.is_object() || value.is_array() => {
                redact_value(&mut value);
                value.to_string()
            }
            _ => line.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n");
    truncate(joined)
}

fn redact_value(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, inner) in map.iter_mut() {
                if is_secret_key(key) {
                    redact_secret(inner);
                } else {
                    redact_value(inner);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(redact_value),
        _ => {}
    }
}

/// Redacts a value known to sit under a secret-looking key: scalars are masked directly;
/// array elements that are scalars are masked too, while nested objects/arrays are still
/// walked with the normal recursion so their own secret-looking keys get masked.
fn redact_secret(value: &mut Value) {
    match value {
        Value::Object(_) => redact_value(value),
        Value::Array(items) => {
            for item in items.iter_mut() {
                if item.is_object() || item.is_array() {
                    redact_value(item);
                } else {
                    *item = Value::String("<redacted>".to_string());
                }
            }
        }
        _ => *value = Value::String("<redacted>".to_string()),
    }
}

fn is_secret_key(key: &str) -> bool {
    let lower = key.to_lowercase();
    SECRET_HINTS.iter().any(|hint| lower.contains(hint))
}

fn truncate(text: String) -> String {
    if text.chars().count() <= MAX_CHARS {
        return text;
    }
    let cut: String = text.chars().take(MAX_CHARS).collect();
    format!("{cut}… [truncated]")
}

/// Rotating file log in the OS log dir plus stdout.
pub fn plugin<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    use tauri_plugin_log::{RotationStrategy, Target, TargetKind};
    tauri_plugin_log::Builder::new()
        .targets([
            Target::new(TargetKind::LogDir { file_name: Some("usage-monitor".into()) }),
            Target::new(TargetKind::Stdout),
        ])
        .max_file_size(1_000_000)
        .rotation_strategy(RotationStrategy::KeepOne)
        .level(log::LevelFilter::Info)
        .level_for("usage_monitor_lib", log::LevelFilter::Debug)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_secret_keys_in_json_lines() {
        let input = r#"{"tokens":{"access_token":"abc","refresh_token":"def","account_id":"x"},"Authorization":"Bearer z","plan":"pro"}"#;
        let out = redact(input);
        assert!(!out.contains("abc") && !out.contains("def") && !out.contains("Bearer z"));
        assert!(out.contains("\"plan\":\"pro\""));
        assert!(out.contains("\"account_id\":\"x\""));
    }

    #[test]
    fn redacts_secret_arrays_of_scalars() {
        let out = redact(r#"{"access_tokens":["abc","def"],"plan":"pro"}"#);
        assert!(!out.contains("abc") && !out.contains("def"));
        assert!(out.contains("\"plan\":\"pro\""));
    }

    #[test]
    fn keeps_plain_text_and_truncates() {
        assert_eq!(redact("hello"), "hello");
        let long = "a".repeat(5000);
        let out = redact(&long);
        assert!(out.ends_with("[truncated]"));
        assert!(out.chars().count() < 2100);
    }
}
