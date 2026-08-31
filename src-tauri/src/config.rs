use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

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

pub fn load_config() -> Config {
    let path = get_config_path();
    if path.exists() {
        let content = fs::read_to_string(&path).unwrap_or_default();
        serde_json::from_str(&content).unwrap_or_else(|_| {
            // Corrupted config, delete and return default
            let _ = fs::remove_file(&path);
            Config::default()
        })
    } else {
        Config::default()
    }
}

pub fn save_config(config: &Config) -> Result<(), String> {
    let path = get_config_path();
    let json = serde_json::to_string_pretty(config)
        .map_err(|e| format!("Serialize error: {}", e))?;
    fs::write(path, json)
        .map_err(|e| format!("Write error: {}", e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_roundtrip() {
        let config = Config {
            project_path: Some("/test/path".to_string()),
            idf_path: Some("/test/idf".to_string()),
            last_serial_port: Some("/dev/test".to_string()),
        };

        save_config(&config).unwrap();
        let loaded = load_config();

        assert_eq!(loaded.project_path, config.project_path);
        assert_eq!(loaded.idf_path, config.idf_path);
        assert_eq!(loaded.last_serial_port, config.last_serial_port);

        // Cleanup
        let _ = fs::remove_file(get_config_path());
    }
}
