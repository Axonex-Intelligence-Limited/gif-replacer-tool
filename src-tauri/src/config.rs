use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct Config {
    pub project_path: Option<String>,
    pub idf_path: Option<String>,
    pub last_serial_port: Option<String>,
}

pub fn get_config_path() -> PathBuf {
    let home = dirs::home_dir().expect("Cannot find home directory");
    home.join(".gif-tool-config.json")
}

/// Reads a config from an explicit path.
///
/// `load_config` is the real entry point; this exists so tests do not read and
/// delete the developer's own `~/.gif-tool-config.json`.
fn load_config_from(path: &Path) -> Config {
    if path.exists() {
        let content = fs::read_to_string(path).unwrap_or_default();
        serde_json::from_str(&content).unwrap_or_else(|_| {
            // Corrupted config, delete and return default
            let _ = fs::remove_file(path);
            Config::default()
        })
    } else {
        Config::default()
    }
}

/// Writes a config to an explicit path. See `load_config_from` for why the path
/// is a parameter.
fn save_config_to(path: &Path, config: &Config) -> Result<(), String> {
    let json = serde_json::to_string_pretty(config)
        .map_err(|e| format!("Serialize error: {}", e))?;
    fs::write(path, json)
        .map_err(|e| format!("Write error: {}", e))?;
    Ok(())
}

pub fn load_config() -> Config {
    load_config_from(&get_config_path())
}

pub fn save_config(config: &Config) -> Result<(), String> {
    save_config_to(&get_config_path(), config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_roundtrip() {
        let path = std::env::temp_dir()
            .join(format!("gif_tool_config_{}.json", std::process::id()));
        let _ = fs::remove_file(&path);

        let config = Config {
            project_path: Some("/test/path".to_string()),
            idf_path: Some("/test/idf".to_string()),
            last_serial_port: Some("/dev/test".to_string()),
        };

        save_config_to(&path, &config).unwrap();
        let loaded = load_config_from(&path);

        assert_eq!(loaded.project_path, config.project_path);
        assert_eq!(loaded.idf_path, config.idf_path);
        assert_eq!(loaded.last_serial_port, config.last_serial_port);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_load_config_from_missing_path_creates_nothing() {
        let path = std::env::temp_dir()
            .join(format!("gif_tool_config_absent_{}.json", std::process::id()));
        let _ = fs::remove_file(&path);

        let config = load_config_from(&path);

        assert!(config.project_path.is_none());
        assert!(!path.exists(), "reading a missing config must not create it");
    }
}
