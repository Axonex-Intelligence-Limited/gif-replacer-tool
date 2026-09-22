// builder.rs - File replacement, build, and flash execution

use std::fs;
use std::process::Command;
use crate::profile::ProfileInfo;

/// Errors that can occur during build/flash operations
#[derive(Debug)]
pub enum BuilderError {
    InvalidEmotion(String, Vec<String>),
    WriteError(String),
    PermissionDenied(String),
    IdfNotFound(String),
    BuildFailed(i32, String),
}

impl std::fmt::Display for BuilderError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::InvalidEmotion(emotion, valid) => {
                write!(f, "'{}' is not a valid emotion for this profile.\nValid emotions: {}",
                    emotion, valid.join(", "))
            },
            Self::WriteError(msg) => write!(f, "Write error: {}", msg),
            Self::PermissionDenied(path) => write!(f, "Cannot write to {}. Check file permissions.", path),
            Self::IdfNotFound(msg) => write!(f, "ESP-IDF not found. {}", msg),
            Self::BuildFailed(code, msg) => write!(f, "Build failed with exit code {}: {}", code, msg),
        }
    }
}

/// Result of build or flash operation
#[derive(Debug)]
pub struct BuildResult {
    pub success: bool,
    pub exit_code: i32,
    pub output: String,
}

/// Replaces a GIF file in the profile
///
/// # Arguments
/// * `profile` - Profile information from detect_active_profile
/// * `emotion` - Target emotion name (must be in profile.emotions)
/// * `content` - New file content (already renamed)
///
/// # Returns
/// * `Ok(())` - File replaced successfully
/// * `Err(BuilderError)` - Validation or write error
/// Validates that `emotion` is one of the profile's slots.
///
/// Split out of `replace_gif_file` because the .gif conversion path needs the
/// check without the write — gif2c.py writes the .c file itself.
pub fn validate_emotion(profile: &ProfileInfo, emotion: &str) -> Result<(), BuilderError> {
    if !profile.emotions.contains(&emotion.to_string()) {
        return Err(BuilderError::InvalidEmotion(
            emotion.to_string(),
            profile.emotions.clone(),
        ));
    }
    Ok(())
}

pub fn replace_gif_file(
    profile: &ProfileInfo,
    emotion: &str,
    content: &str,
) -> Result<(), BuilderError> {
    validate_emotion(profile, emotion)?;

    // Construct target path
    let target_path = profile.profile_path.join("gif").join(format!("{}.c", emotion));

    // Write content to file with proper error handling
    fs::write(&target_path, content).map_err(|e| {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            BuilderError::PermissionDenied(target_path.to_string_lossy().to_string())
        } else {
            BuilderError::WriteError(format!("{}: {}", target_path.to_string_lossy(), e))
        }
    })?;

    Ok(())
}

/// Resolves the ESP-IDF install to use for build/flash. The firmware requires
/// ESP-IDF **v5.5.2** — v6.2 reshapes the LCD panel struct and breaks the
/// build — so prefer the known-good checkout at `~/esp/esp-idf-v5.5.2` and only
/// fall back to `$IDF_PATH` when that's absent.
fn resolve_idf_path() -> Result<String, BuilderError> {
    if let Ok(home) = std::env::var("HOME") {
        let known = format!("{}/esp/esp-idf-v5.5.2", home);
        if std::path::Path::new(&known).join("export.sh").exists() {
            return Ok(known);
        }
    }
    std::env::var("IDF_PATH").map_err(|_| {
        BuilderError::IdfNotFound(
            "ESP-IDF v5.5.2 not found. Install it at ~/esp/esp-idf-v5.5.2 or set $IDF_PATH.".to_string(),
        )
    })
}

/// Runs ESP-IDF build command
///
/// # Arguments
/// * `project_path` - Path to EmotionDisplay project
///
/// # Returns
/// * `Ok(BuildResult)` - Build completed (check success field)
/// * `Err(BuilderError)` - Command execution failed
pub fn run_build_sync(project_path: &str) -> Result<BuildResult, BuilderError> {
    let idf_path = resolve_idf_path()?;

    // Run build command. Unset inherited IDF env vars first: a stale
    // IDF_PYTHON_ENV_PATH from a prior v6.2 activation would otherwise make
    // `idf.py` run under the wrong python and halt with "Run fullclean".
    let output = Command::new("sh")
        .arg("-c")
        .arg(format!(
            "unset IDF_PATH IDF_PYTHON_ENV_PATH VIRTUAL_ENV; source {}/export.sh && idf.py build",
            idf_path
        ))
        .current_dir(project_path)
        .output()
        .map_err(|e| BuilderError::BuildFailed(-1, e.to_string()))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let combined = format!("{}\n{}", stdout, stderr);

    Ok(BuildResult {
        success: output.status.success(),
        exit_code: output.status.code().unwrap_or(-1),
        output: combined,
    })
}

/// Runs ESP-IDF flash command
///
/// # Arguments
/// * `project_path` - Path to EmotionDisplay project
/// * `serial_port` - Serial port path (e.g., /dev/cu.usbserial-210)
///
/// # Returns
/// * `Ok(BuildResult)` - Flash completed (check success field)
/// * `Err(BuilderError)` - Command execution failed
pub fn run_flash_sync(project_path: &str, serial_port: &str) -> Result<BuildResult, BuilderError> {
    let idf_path = resolve_idf_path()?;

    // Verify serial port exists
    if !std::path::Path::new(serial_port).exists() {
        return Err(BuilderError::WriteError(format!(
            "Device not found at {}. Check connection and try: ls /dev/cu.*",
            serial_port
        )));
    }

    // Run flash command (same env cleanup as build — see run_build_sync).
    let output = Command::new("sh")
        .arg("-c")
        .arg(format!(
            "unset IDF_PATH IDF_PYTHON_ENV_PATH VIRTUAL_ENV; source {}/export.sh && idf.py -p {} flash",
            idf_path, serial_port
        ))
        .current_dir(project_path)
        .output()
        .map_err(|e| BuilderError::BuildFailed(-1, e.to_string()))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let combined = format!("{}\n{}", stdout, stderr);

    // Check for common error patterns and provide helpful messages
    let result_output = if combined.contains("Permission denied") {
        format!(
            "{}\n\nHint: Add your user to dialout group: sudo usermod -a -G dialout $USER",
            combined
        )
    } else if combined.contains("Device or resource busy") {
        format!(
            "{}\n\nHint: Close any serial monitors (screen, minicom, idf.py monitor)",
            combined
        )
    } else {
        combined
    };

    Ok(BuildResult {
        success: output.status.success(),
        exit_code: output.status.code().unwrap_or(-1),
        output: result_output,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::ProfileInfo;
    use std::path::PathBuf;

    #[test]
    fn test_validate_emotion_rejects_unknown() {
        let profile = ProfileInfo {
            profile: "test".to_string(),
            emotions: vec!["angry".to_string(), "happy".to_string()],
            symbols: vec!["angry".to_string(), "happy".to_string()],
            profile_path: PathBuf::from("/tmp/test"),
        };

        let result = validate_emotion(&profile, "invalid");
        assert!(matches!(result, Err(BuilderError::InvalidEmotion(_, _))));
    }

    #[test]
    fn test_validate_emotion_accepts_known() {
        let profile = ProfileInfo {
            profile: "test".to_string(),
            emotions: vec!["angry".to_string(), "happy".to_string()],
            symbols: vec!["angry".to_string(), "happy".to_string()],
            profile_path: PathBuf::from("/tmp/test_nonexistent"),
        };

        assert!(validate_emotion(&profile, "angry").is_ok());
    }

    #[test]
    fn test_validate_emotion_writes_nothing() {
        let profile = ProfileInfo {
            profile: "test".to_string(),
            emotions: vec!["angry".to_string()],
            symbols: vec!["angry".to_string()],
            profile_path: PathBuf::from("/tmp/gif_tool_validate_emotion_probe"),
        };

        let _ = fs::remove_dir_all(&profile.profile_path);
        assert!(validate_emotion(&profile, "angry").is_ok());
        assert!(
            !profile.profile_path.exists(),
            "validate_emotion must not create or write anything"
        );
    }
}
