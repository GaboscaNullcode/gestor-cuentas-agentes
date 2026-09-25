pub mod accounts;
pub mod cache;
pub mod detect;
pub mod settings;

use serde::{de::DeserializeOwned, Serialize};
use std::fs;
use std::path::Path;

/// Reads a JSON file; a missing file gives the default, a corrupt one is moved to `*.corrupt`
/// so the next save does not silently destroy the user's data.
pub fn read_json<T: DeserializeOwned + Default>(path: &Path) -> T {
    let Ok(text) = fs::read_to_string(path) else { return T::default() };
    match serde_json::from_str(&text) {
        Ok(value) => value,
        Err(e) => {
            log::warn!("{} is unreadable ({e}); moving it aside", path.display());
            let _ = fs::rename(path, path.with_extension("json.corrupt"));
            T::default()
        }
    }
}

pub fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(value).map_err(std::io::Error::other)?;
    fs::write(&tmp, bytes)?;
    fs::rename(tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::settings::Settings;

    #[test]
    fn missing_file_yields_default_and_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("settings.json");
        let loaded: Settings = read_json(&path);
        assert_eq!(loaded, Settings::default());
        let custom = Settings { interval_minutes: 7, ..Default::default() };
        write_json_atomic(&path, &custom).unwrap();
        assert_eq!(read_json::<Settings>(&path), custom);
    }

    #[test]
    fn corrupt_file_is_moved_aside() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("accounts.json");
        std::fs::write(&path, "{not json").unwrap();
        let loaded: Vec<crate::model::Account> = read_json(&path);
        assert!(loaded.is_empty());
        assert!(dir.path().join("accounts.json.corrupt").exists());
    }
}
