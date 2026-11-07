//! Where Reduct keeps its settings and logs.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use reduct_common::settings::Settings;
use windows::Storage::ApplicationData;

const SETTINGS_FILE: &str = "settings.ini";

/// The app's data folder, created if needed.
///
/// With package identity this is the package's `LocalState` folder
/// (`ApplicationData::Current().LocalFolder()`). Without identity, as in an
/// unpackaged run, `ApplicationData::Current()` fails and the folder is
/// `%LOCALAPPDATA%\Reduct`.
pub fn data_dir() -> PathBuf {
    let dir = package_local_folder().unwrap_or_else(unpackaged_dir);
    let _ = fs::create_dir_all(&dir);
    dir
}

fn package_local_folder() -> Option<PathBuf> {
    let path = ApplicationData::Current().ok()?.LocalFolder().ok()?.Path().ok()?;
    if path.is_empty() {
        return None;
    }
    Some(PathBuf::from(path.to_os_string()))
}

fn unpackaged_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Reduct")
}

/// Reads `data_dir()\settings.ini`; a missing or unreadable file gives the defaults.
pub fn load_settings() -> Settings {
    load_settings_in(&data_dir())
}

/// Reads `<dir>\settings.ini`; a missing or unreadable file gives the defaults.
pub fn load_settings_in(dir: &Path) -> Settings {
    load_settings_from(&dir.join(SETTINGS_FILE))
}

/// Writes `data_dir()\settings.ini`.
pub fn save_settings(settings: &Settings) -> io::Result<()> {
    save_settings_to(&data_dir().join(SETTINGS_FILE), settings)
}

fn load_settings_from(path: &Path) -> Settings {
    fs::read_to_string(path)
        .map(|text| Settings::parse(&text))
        .unwrap_or_default()
}

fn save_settings_to(path: &Path, settings: &Settings) -> io::Result<()> {
    fs::write(path, settings.serialize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_through_a_file() {
        let path = std::env::temp_dir().join(format!("reduct-settings-{}.ini", std::process::id()));
        let settings = Settings { quality: 88, keep_file_date: true, ..Settings::default() };
        save_settings_to(&path, &settings).unwrap();
        let back = load_settings_from(&path);
        let _ = fs::remove_file(&path);
        assert_eq!(back, settings);
    }

    #[test]
    fn missing_settings_file_gives_defaults() {
        let path = std::env::temp_dir().join("reduct-settings-does-not-exist.ini");
        assert_eq!(load_settings_from(&path), Settings::default());
    }

    #[test]
    fn data_dir_without_identity_is_created() {
        // Test binaries have no package identity, so this is the fallback.
        let dir = data_dir();
        assert!(dir.ends_with("Reduct"), "{}", dir.display());
        assert!(dir.is_dir());
    }
}
