// builder.rs - File replacement, build, and flash execution

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;
use regex::Regex;
use serde::Serialize;
use crate::config;
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

/// The file that proves a directory is a real IDF checkout.
fn idf_marker() -> &'static str {
    if cfg!(windows) { "export.bat" } else { "export.sh" }
}

/// Where to look for ESP-IDF v5.5.2, most likely first.
///
/// `configured` leads on purpose: on Windows the checkout is somewhere none of
/// the probes may know about, and the tool's own setting is the escape hatch.
fn idf_candidates(configured: Option<&str>) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = configured.map(PathBuf::from).into_iter().collect();

    #[cfg(windows)]
    {
        // IDF_TOOLS_PATH, if the user's shell has it, points straight at the
        // framework: <root>\frameworks\esp-idf-v5.5.2.
        if let Ok(tools) = std::env::var("IDF_TOOLS_PATH") {
            v.push(PathBuf::from(&tools).join("frameworks").join("esp-idf-v5.5.2"));
        }
        // The official installer's defaults: framework under \frameworks\,
        // toolchains and python env at the root itself.
        v.push(PathBuf::from(r"C:\Espressif\frameworks\esp-idf-v5.5.2"));
        if let Ok(profile) = std::env::var("USERPROFILE") {
            v.push(PathBuf::from(profile).join("esp").join("esp-idf-v5.5.2"));
        }
        v.push(PathBuf::from(r"C:\esp\esp-idf-v5.5.2"));
    }
    #[cfg(not(windows))]
    {
        if let Ok(home) = std::env::var("HOME") {
            v.push(PathBuf::from(home).join("esp").join("esp-idf-v5.5.2"));
        }
    }

    v
}

/// The IDF_TOOLS_PATH a checkout needs, derived from where it sits.
///
/// On Windows the tools are NOT next to the framework. `export.bat` resolves
/// toolchains and the python env under IDF_TOOLS_PATH and falls back to
/// `%USERPROFILE%\.espressif` when it is unset — where an installer-provided
/// checkout's tools are not. The export then "succeeds" with no toolchain on
/// PATH, and the build dies with a bare `'idf.py' is not recognized`, which
/// says nothing about the real cause. The installer sets this only inside its
/// "ESP-IDF CMD/PowerShell" shortcuts; the app inherits no such thing.
#[cfg(windows)]
fn idf_tools_path_for(idf_path: &Path) -> Option<PathBuf> {
    let frameworks = idf_path.parent()?;                    // <root>\frameworks
    if frameworks.file_name()?.to_ascii_lowercase() != "frameworks" {
        return None;
    }
    frameworks.parent().map(Path::to_path_buf)              // <root>
}

/// Sibling checkouts that exist but are the wrong version. Worth naming:
/// "not found" reads as "you never installed it" when the real answer is
/// usually "you installed v6.2".
fn other_versions(candidates: &[PathBuf]) -> Vec<String> {
    let mut found = Vec::new();
    for c in candidates {
        let Some(parent) = c.parent() else { continue };
        let Ok(entries) = fs::read_dir(parent) else { continue };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with("esp-idf-v") {
                found.push(e.path().to_string_lossy().to_string());
            }
        }
    }
    found.sort();
    found.dedup();
    found
}

/// The "not found" error. Pure, so the wording is testable without a real
/// ESP-IDF install or a mutated environment.
fn idf_not_found_message(candidates: &[PathBuf]) -> String {
    let mut msg = String::from("ESP-IDF v5.5.2 not found.\n\nLooked in:");
    for c in candidates {
        msg.push_str(&format!("\n  {}", c.display()));
    }
    msg.push_str(
        "\n\nThis project needs 5.5.2 — v6.x reshapes the LCD panel struct and does not build.",
    );

    let others = other_versions(candidates);
    if !others.is_empty() {
        msg.push_str("\nAlso present, wrong version:");
        for o in others {
            msg.push_str(&format!("\n  {}", o));
        }
    }

    if cfg!(windows) {
        msg.push_str("\n\nInstall 5.5.2, or set \"idf_path\" in %USERPROFILE%\\.gif-tool-config.json.");
    } else {
        msg.push_str("\n\nInstall 5.5.2, or set \"idf_path\" in ~/.gif-tool-config.json.");
    }

    msg
}

/// Resolves the ESP-IDF install to use for build/flash. The firmware requires
/// ESP-IDF **v5.5.2** — v6.2 reshapes the LCD panel struct and breaks the
/// build — so prefer the known-good checkouts and only fall back to `$IDF_PATH`.
///
/// `configured` is the tool's own `idf_path` setting, tried first. Taking it as
/// a parameter rather than reading the config here keeps this function pure and
/// its behaviour testable; the callers do the I/O.
fn resolve_idf_path(configured: Option<&str>) -> Result<String, BuilderError> {
    let candidates = idf_candidates(configured);

    for c in &candidates {
        if c.join(idf_marker()).exists() {
            return Ok(c.to_string_lossy().to_string());
        }
    }

    if let Ok(idf) = std::env::var("IDF_PATH") {
        if !idf.is_empty() {
            return Ok(idf);
        }
    }

    Err(BuilderError::IdfNotFound(idf_not_found_message(&candidates)))
}

/// The ESP-IDF tools root for the checkout the build path would use, if any.
///
/// Conversion uses it to find the IDF python env (spec §1d). IDF being absent
/// is not a conversion failure, so this returns `Option` rather than `Result`.
#[cfg(windows)]
pub fn resolved_idf_tools_path() -> Option<PathBuf> {
    let configured = config::load_config().idf_path;
    let idf = resolve_idf_path(configured.as_deref()).ok()?;
    idf_tools_path_for(Path::new(&idf))
}

/// Unix has no separate tools root — `export.sh` finds its own.
#[cfg(not(windows))]
pub fn resolved_idf_tools_path() -> Option<PathBuf> {
    None
}

/// App partition size from EmotionDisplay/partitions.csv:
/// `factory, app, factory, 0x10000, 0x500000`.
pub const APP_PARTITION_BYTES: u64 = 0x500000;

/// How much room is left, and whether the number can be trusted.
#[derive(Debug, Serialize)]
#[serde(tag = "state", content = "bytes")]
pub enum Headroom {
    /// No build artifact at all — the project has never been built.
    NeverBuilt,
    /// The newest artifact is older than the sources it was built from, so its
    /// size describes a different tree. Reporting it as current is how a stale
    /// 6 MB binary once got read as "0 KB free" and sent a build to its death.
    Stale,
    /// Free bytes per the newest build. Negative means that build overflowed
    /// the partition — ESP-IDF writes the .bin before checking whether it fits.
    Known(i64),
}

#[derive(Debug)]
pub struct BudgetCheck {
    pub incoming_bytes: u64,
    pub current_bytes: u64,
    /// Free space per the newest build, or why we cannot state one.
    pub headroom: Headroom,
    pub delta_bytes: i64,
    pub overflows: bool,
}

/// Pulls `.data_size` out of a generated LVGL .c file. Returns None when the
/// slot does not exist yet, or the file has no recognisable data_size.
fn read_current_size(c_file: &std::path::Path) -> Option<u64> {
    let text = fs::read_to_string(c_file).ok()?;
    let re = Regex::new(r"\.data_size\s*=\s*(\d+)").ok()?;
    re.captures(&text)?.get(1)?.as_str().parse().ok()
}

/// (size, mtime) of the newest application binary in `build/`. Excludes the
/// bootloader and partition table — the largest remaining .bin is the app image.
fn newest_app_binary(project: &std::path::Path) -> Option<(u64, SystemTime)> {
    fs::read_dir(project.join("build"))
        .ok()?
        .flatten()
        .filter(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            name.ends_with(".bin")
                && !name.contains("bootloader")
                && !name.contains("partition-table")
        })
        .filter_map(|e| {
            let md = e.metadata().ok()?;
            Some((md.len(), md.modified().ok()?))
        })
        .max_by_key(|(size, _)| *size)
}

/// Newest mtime under `dir`, skipping build output and VCS metadata.
fn newest_mtime(dir: &std::path::Path, depth: usize) -> Option<SystemTime> {
    if depth > 6 {
        return None;
    }

    let mut newest: Option<SystemTime> = None;
    for entry in fs::read_dir(dir).ok()?.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name == "build" || name == ".git" {
            continue;
        }

        let Ok(md) = entry.metadata() else { continue };
        let m = if md.is_dir() {
            newest_mtime(&entry.path(), depth + 1)
        } else {
            md.modified().ok()
        };

        if let Some(m) = m {
            if newest.map_or(true, |n| m > n) {
                newest = Some(m);
            }
        }
    }
    newest
}

/// Newest mtime among the inputs that actually change the firmware image.
///
/// Deliberately a fixed list rather than a whole-tree walk: generated files
/// such as `dependencies.lock` would otherwise make every build look stale.
fn newest_source_mtime(project: &std::path::Path) -> Option<SystemTime> {
    let mut newest: Option<SystemTime> = None;

    for name in ["sdkconfig", "sdkconfig.defaults", "partitions.csv"] {
        if let Ok(m) = fs::metadata(project.join(name)).and_then(|md| md.modified()) {
            if newest.map_or(true, |n| m > n) {
                newest = Some(m);
            }
        }
    }

    if let Some(m) = newest_mtime(&project.join("main"), 0) {
        if newest.map_or(true, |n| m > n) {
            newest = Some(m);
        }
    }

    newest
}

/// Free space in the app partition according to the newest build, or the
/// reason that number cannot be trusted.
fn measure_headroom(project: &std::path::Path) -> Headroom {
    let Some((size, bin_mtime)) = newest_app_binary(project) else {
        return Headroom::NeverBuilt;
    };

    // A .bin older than the sources describes a previous tree.
    if let Some(src_mtime) = newest_source_mtime(project) {
        if bin_mtime < src_mtime {
            return Headroom::Stale;
        }
    }

    Headroom::Known(APP_PARTITION_BYTES as i64 - size as i64)
}

/// Warns when replacing a slot with a much larger GIF would overflow the app
/// partition. Advisory only — the caller decides whether to proceed, since
/// freeing space first (nullptr gif_table entries) is a legitimate answer.
pub fn check_flash_budget(
    project_path: &str,
    profile: &ProfileInfo,
    emotion: &str,
    incoming_bytes: u64,
) -> Result<BudgetCheck, BuilderError> {
    let c_file = profile.profile_path.join("gif").join(format!("{}.c", emotion));
    let current_bytes = read_current_size(&c_file).unwrap_or(0);
    let delta_bytes = incoming_bytes as i64 - current_bytes as i64;
    let headroom = measure_headroom(std::path::Path::new(project_path));

    // Overflows only when we have a trustworthy number and the delta exceeds
    // it. Never-built and stale baselines report overflows=false; the frontend
    // says the headroom is unknown rather than inventing one.
    let overflows = match &headroom {
        Headroom::Known(h) => delta_bytes > *h,
        _ => false,
    };

    Ok(BudgetCheck {
        incoming_bytes,
        current_bytes,
        headroom,
        delta_bytes,
        overflows,
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
    let idf_path = resolve_idf_path(config::load_config().idf_path.as_deref())?;

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
    let idf_path = resolve_idf_path(config::load_config().idf_path.as_deref())?;

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

    #[test]
    fn test_read_current_size_parses_data_size() {
        let dir = std::env::temp_dir().join(format!("gif_tool_budget_{}", std::process::id()));
        let gif_dir = dir.join("gif");
        fs::create_dir_all(&gif_dir).unwrap();
        fs::write(
            gif_dir.join("angry.c"),
            "const lv_img_dsc_t angry = {\n  .data_size = 70149,\n};\n",
        )
        .unwrap();

        assert_eq!(read_current_size(&gif_dir.join("angry.c")), Some(70149));
    }

    #[test]
    fn test_read_current_size_returns_none_for_new_slot() {
        let missing = std::env::temp_dir().join("gif_tool_budget_absent.c");
        let _ = fs::remove_file(&missing);
        assert_eq!(read_current_size(&missing), None);
    }

    #[test]
    fn test_measure_headroom_uses_largest_app_binary() {
        let dir = std::env::temp_dir().join(format!("gif_tool_build_{}", std::process::id()));
        let build = dir.join("build");
        fs::create_dir_all(&build).unwrap();
        fs::write(build.join("bootloader.bin"), vec![0u8; 100]).unwrap();
        fs::write(build.join("partition-table.bin"), vec![0u8; 100]).unwrap();
        fs::write(build.join("lvgl_porting.bin"), vec![0u8; 4_595_472]).unwrap();

        match measure_headroom(&dir) {
            Headroom::Known(h) => assert_eq!(h, APP_PARTITION_BYTES as i64 - 4_595_472),
            other => panic!("expected Known, got {:?}", other),
        }
    }

    #[test]
    fn test_measure_headroom_reports_over_budget_as_negative() {
        let dir = std::env::temp_dir().join(format!("gif_tool_over_{}", std::process::id()));
        let build = dir.join("build");
        fs::create_dir_all(&build).unwrap();
        // A build that failed the partition check still leaves its .bin behind.
        let oversize = APP_PARTITION_BYTES + 1_000;
        fs::write(build.join("lvgl_porting.bin"), vec![0u8; oversize as usize]).unwrap();

        match measure_headroom(&dir) {
            Headroom::Known(h) => assert_eq!(
                h, -1_000,
                "an oversized artifact must report negative headroom, not clamp to 0"
            ),
            other => panic!("expected Known, got {:?}", other),
        }
    }

    #[test]
    fn test_over_budget_baseline_forces_overflow() {
        let dir = std::env::temp_dir().join(format!("gif_tool_overflag_{}", std::process::id()));
        let build = dir.join("build");
        fs::create_dir_all(&build).unwrap();
        fs::write(
            build.join("lvgl_porting.bin"),
            vec![0u8; (APP_PARTITION_BYTES + 500_000) as usize],
        )
        .unwrap();

        let profile = ProfileInfo {
            profile: "test".to_string(),
            emotions: vec!["cool".to_string()],
            symbols: vec!["cool".to_string()],
            profile_path: dir.clone(),
        };

        // Even a tiny incoming file overflows once the baseline is over budget.
        let check = check_flash_budget(dir.to_str().unwrap(), &profile, "cool", 1).unwrap();
        assert!(matches!(check.headroom, Headroom::Known(h) if h < 0));
        assert!(check.overflows);
    }

    #[test]
    fn test_measure_headroom_never_built() {
        let dir = std::env::temp_dir().join(format!("gif_tool_nobuild_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        assert!(matches!(measure_headroom(&dir), Headroom::NeverBuilt));
    }

    #[test]
    fn test_measure_headroom_stale_when_sources_are_newer() {
        let dir = std::env::temp_dir().join(format!("gif_tool_stale_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("build")).unwrap();
        fs::create_dir_all(dir.join("main")).unwrap();

        fs::write(dir.join("build").join("lvgl_porting.bin"), vec![0u8; 100_000]).unwrap();
        // Touch a source after the artifact, so the build is older than the tree.
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(dir.join("main").join("main.cpp"), b"// edited after the build").unwrap();

        assert!(
            matches!(measure_headroom(&dir), Headroom::Stale),
            "a .bin older than main/ must not be reported as the current baseline"
        );
    }

    #[test]
    fn test_measure_headroom_known_when_build_is_newest() {
        let dir = std::env::temp_dir().join(format!("gif_tool_fresh_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("main")).unwrap();
        fs::write(dir.join("main").join("main.cpp"), b"// source").unwrap();

        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::create_dir_all(dir.join("build")).unwrap();
        fs::write(
            dir.join("build").join("lvgl_porting.bin"),
            vec![0u8; (APP_PARTITION_BYTES - 3_000) as usize],
        )
        .unwrap();

        match measure_headroom(&dir) {
            Headroom::Known(h) => assert_eq!(h, 3_000),
            other => panic!("expected Known, got {:?}", other),
        }
    }

    #[test]
    fn test_idf_candidates_put_configured_first() {
        let candidates = idf_candidates(Some("/opt/my-idf"));
        assert_eq!(candidates[0], PathBuf::from("/opt/my-idf"));
    }

    #[test]
    fn test_idf_candidates_never_empty_without_configured() {
        assert!(!idf_candidates(None).is_empty());
    }

    #[test]
    fn test_resolve_idf_path_accepts_a_checkout_with_the_marker() {
        let dir = std::env::temp_dir().join(format!("gif_tool_idf_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(idf_marker()), b"# stub").unwrap();

        let resolved = resolve_idf_path(Some(dir.to_str().unwrap())).unwrap();
        assert_eq!(resolved, dir.to_str().unwrap());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_idf_not_found_message_names_every_candidate() {
        let msg = idf_not_found_message(&[
            PathBuf::from("/a/esp-idf-v5.5.2"),
            PathBuf::from("/b/esp-idf-v5.5.2"),
        ]);
        assert!(msg.contains("/a/esp-idf-v5.5.2"));
        assert!(msg.contains("/b/esp-idf-v5.5.2"));
        assert!(msg.contains("5.5.2"));
    }

    #[test]
    fn test_idf_not_found_message_names_the_config_escape_hatch() {
        let msg = idf_not_found_message(&[]);
        assert!(msg.contains("gif-tool-config.json"));
        assert!(msg.contains("idf_path"));
    }

    #[test]
    fn test_other_versions_finds_siblings() {
        let root = std::env::temp_dir().join(format!("gif_tool_vers_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let frameworks = root.join("frameworks");
        fs::create_dir_all(frameworks.join("esp-idf-v6.2")).unwrap();
        fs::create_dir_all(frameworks.join("esp-idf-v5.5.2")).unwrap();

        let found = other_versions(&[frameworks.join("esp-idf-v5.5.2")]);

        assert!(
            found.iter().any(|p| p.contains("esp-idf-v6.2")),
            "a sibling checkout of the wrong version must be named: {:?}",
            found
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(windows)]
    #[test]
    fn test_idf_candidates_include_the_windows_installer_layout() {
        let candidates = idf_candidates(None);
        assert!(candidates.contains(&PathBuf::from(r"C:\Espressif\frameworks\esp-idf-v5.5.2")));
        assert!(candidates.contains(&PathBuf::from(r"C:\esp\esp-idf-v5.5.2")));
    }

    #[cfg(windows)]
    #[test]
    fn test_idf_tools_path_from_installer_layout() {
        let idf = Path::new(r"C:\Espressif\frameworks\esp-idf-v5.5.2");
        assert_eq!(idf_tools_path_for(idf), Some(PathBuf::from(r"C:\Espressif")));
    }

    #[cfg(windows)]
    #[test]
    fn test_idf_tools_path_none_when_not_under_frameworks() {
        assert_eq!(idf_tools_path_for(Path::new(r"D:\idf\esp-idf-v5.5.2")), None);
    }
}
