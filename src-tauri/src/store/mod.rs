pub mod accounts;
pub mod cache;
pub mod detect;
pub mod settings;

use serde::{de::DeserializeOwned, Serialize};
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

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

/// Writes to a unique temp file next to `path` and renames it into place. The temp name is
/// unique per write (pid + counter), so concurrent saves never share one inode.
pub fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let n = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = path.with_extension(format!("json.{}.{n}.tmp", std::process::id()));
    let bytes = serde_json::to_vec_pretty(value).map_err(std::io::Error::other)?;
    if let Err(e) = fs::write(&tmp, bytes).and_then(|()| fs::rename(&tmp, path)) {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
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
    fn sequential_writes_keep_the_last_content_and_leave_no_temp_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        write_json_atomic(&path, &Settings { interval_minutes: 3, ..Default::default() }).unwrap();
        write_json_atomic(&path, &Settings { interval_minutes: 9, ..Default::default() }).unwrap();
        assert_eq!(read_json::<Settings>(&path).interval_minutes, 9);
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["settings.json".to_string()]);
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
