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
pub fn replace_gif_file(
    profile: &ProfileInfo,
    emotion: &str,
    content: &str,
) -> Result<(), BuilderError> {
    // Validate emotion is in profile.emotions list
    if !profile.emotions.contains(&emotion.to_string()) {
        return Err(BuilderError::InvalidEmotion(
            emotion.to_string(),
            profile.emotions.clone(),
        ));
    }

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

/// Runs ESP-IDF build command
///
/// # Arguments
/// * `project_path` - Path to EmotionDisplay project
///
/// # Returns
/// * `Ok(BuildResult)` - Build completed (check success field)
/// * `Err(BuilderError)` - Command execution failed
pub fn run_build_sync(project_path: &str) -> Result<BuildResult, BuilderError> {
    // Check $IDF_PATH environment variable
    let idf_path = std::env::var("IDF_PATH").map_err(|_| {
        BuilderError::IdfNotFound(
            "Set $IDF_PATH or run: source ~/esp/esp-idf/export.sh".to_string(),
        )
    })?;

    // Run build command
    let output = Command::new("sh")
        .arg("-c")
        .arg(format!("source {}/export.sh && idf.py build", idf_path))
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
    // Check $IDF_PATH environment variable
    let idf_path = std::env::var("IDF_PATH").map_err(|_| {
        BuilderError::IdfNotFound(
            "Set $IDF_PATH or run: source ~/esp/esp-idf/export.sh".to_string(),
        )
    })?;

    // Verify serial port exists
    if !std::path::Path::new(serial_port).exists() {
        return Err(BuilderError::WriteError(format!(
            "Device not found at {}. Check connection and try: ls /dev/cu.*",
            serial_port
        )));
    }

    // Run flash command
    let output = Command::new("sh")
        .arg("-c")
        .arg(format!(
            "source {}/export.sh && idf.py -p {} flash",
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
    fn test_invalid_emotion() {
        let profile = ProfileInfo {
            profile: "test".to_string(),
            emotions: vec!["angry".to_string(), "happy".to_string()],
            profile_path: PathBuf::from("/tmp/test"),
        };

        let result = replace_gif_file(&profile, "invalid", "content");
        assert!(matches!(result, Err(BuilderError::InvalidEmotion(_, _))));
    }

    #[test]
    fn test_valid_emotion_check() {
        let profile = ProfileInfo {
            profile: "test".to_string(),
            emotions: vec!["angry".to_string(), "happy".to_string()],
            profile_path: PathBuf::from("/tmp/test_nonexistent"),
        };

        // This will fail at write stage (directory doesn't exist), but emotion validation passes
        let result = replace_gif_file(&profile, "angry", "test content");
        // Should not be InvalidEmotion error
        assert!(!matches!(result, Err(BuilderError::InvalidEmotion(_, _))));
    }
}
