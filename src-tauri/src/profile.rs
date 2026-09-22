// profile.rs - Profile detection module
// This is the REFERENCE IMPLEMENTATION for Sonnet to follow

use std::fs;
use std::path::{Path, PathBuf};
use regex::Regex;

/// Errors that can occur during profile detection
#[derive(Debug)]
pub enum ProfileError {
    ProjectNotFound,
    NoSdkConfig,
    NoProfileConfigured,
    ProfileFolderMissing(String),
    MalformedGifTable(String),
    WriteError(String),
}

impl std::fmt::Display for ProfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::ProjectNotFound => write!(f, "Project path not found"),
            Self::NoSdkConfig => write!(f, "Not an EmotionDisplay project. sdkconfig.defaults not found."),
            Self::NoProfileConfigured => write!(f, "No profile configured. Run 'idf.py menuconfig' first."),
            Self::ProfileFolderMissing(p) => write!(f, "Profile '{}' configured but main/{}/ folder missing.", p, p),
            Self::MalformedGifTable(path) => write!(f, "Cannot parse {}. File may be corrupted.", path),
            Self::WriteError(e) => write!(f, "Failed to write config: {}", e),
        }
    }
}

/// Information about a detected profile
#[derive(Debug)]
pub struct ProfileInfo {
    pub profile: String,
    pub emotions: Vec<String>,
    pub symbols: Vec<String>,
    pub profile_path: PathBuf,
}

/// Detects the active profile in an EmotionDisplay project
///
/// # Arguments
/// * `project_path` - Path to the EmotionDisplay project root
///
/// # Returns
/// * `Ok(ProfileInfo)` - Profile name, emotions list, and profile path
/// * `Err(ProfileError)` - Specific error if detection fails
///
/// # Example
/// ```
/// let info = detect_active_profile("/path/to/EmotionDisplay")?;
/// println!("Profile: {}", info.profile);
/// println!("Emotions: {:?}", info.emotions);
/// ```
pub fn detect_active_profile(project_path: &str) -> Result<ProfileInfo, ProfileError> {
    let project = Path::new(project_path);

    // Check project exists
    if !project.exists() {
        return Err(ProfileError::ProjectNotFound);
    }

    // Read sdkconfig.defaults
    let sdkconfig = project.join("sdkconfig.defaults");
    if !sdkconfig.exists() {
        return Err(ProfileError::NoSdkConfig);
    }

    let content = fs::read_to_string(&sdkconfig)
        .map_err(|_| ProfileError::NoSdkConfig)?;

    // Prefer the explicit name override (how clones are activated); fall back to
    // the legacy choice symbol for projects the tool hasn't touched yet.
    let name_re = Regex::new(r#"(?m)^\s*CONFIG_GIF_PROFILE_NAME\s*=\s*"?([A-Za-z0-9_]+)"?"#).unwrap();
    let profile = name_re.captures(&content)
        .and_then(|cap| cap.get(1))
        .map(|m| m.as_str().to_string())
        .or_else(|| {
            let choice_re = Regex::new(r"(?m)^\s*CONFIG_GIF_PROFILE_(\w+)=y").unwrap();
            choice_re.captures(&content)
                .and_then(|cap| cap.get(1))
                .map(|m| m.as_str().to_lowercase())
        })
        .ok_or(ProfileError::NoProfileConfigured)?;

    // Verify profile folder exists
    let profile_path = project.join("main").join(&profile);
    if !profile_path.exists() {
        return Err(ProfileError::ProfileFolderMissing(profile.clone()));
    }

    // Parse gif_profile.h for emotion names (gif_table keys) and symbol names
    // (extern const lv_img_dsc_t declarations).
    let gif_header = profile_path.join("gif_profile.h");
    let emotions = parse_gif_table(&gif_header)
        .map_err(|_| ProfileError::MalformedGifTable(
            gif_header.to_string_lossy().to_string()
        ))?;
    let symbols = parse_gif_symbols(&gif_header)
        .map_err(|_| ProfileError::MalformedGifTable(
            gif_header.to_string_lossy().to_string()
        ))?;

    Ok(ProfileInfo {
        profile,
        emotions,
        symbols,
        profile_path
    })
}

/// Parses the gif_table[] array from gif_profile.h
fn parse_gif_table(header_path: &Path) -> Result<Vec<String>, ()> {
    let content = fs::read_to_string(header_path).map_err(|_| ())?;

    // Find gif_table[] array entries: { "emotion", ... }
    let table_re = Regex::new(r#"\{\s*"(\w+)"\s*,"#).unwrap();
    let emotions: Vec<String> = table_re.captures_iter(&content)
        .filter_map(|cap| cap.get(1).map(|m| m.as_str().to_string()))
        .collect();

    if emotions.is_empty() {
        return Err(());
    }

    Ok(emotions)
}

/// Parses the `extern const lv_img_dsc_t <name>;` declarations — the symbol
/// names valid for `#define GIF_PROFILE_DEFAULT <name>`. This is a distinct
/// list from `parse_gif_table`: the table maps keys like "startup"/"standby"
/// onto a handful of symbols, and only symbols are valid defaults.
fn parse_gif_symbols(header_path: &Path) -> Result<Vec<String>, ()> {
    let content = fs::read_to_string(header_path).map_err(|_| ())?;

    let symbol_re = Regex::new(r"extern\s+const\s+lv_img_dsc_t\s+(\w+)\s*;").unwrap();
    let symbols: Vec<String> = symbol_re.captures_iter(&content)
        .filter_map(|cap| cap.get(1).map(|m| m.as_str().to_string()))
        .collect();

    if symbols.is_empty() {
        return Err(());
    }

    Ok(symbols)
}

/// Lists all available profiles in the project by scanning main/ directory
pub fn list_available_profiles(project_path: &str) -> Result<Vec<String>, ProfileError> {
    let project = Path::new(project_path);

    if !project.exists() {
        return Err(ProfileError::ProjectNotFound);
    }

    let main_dir = project.join("main");
    if !main_dir.exists() {
        return Ok(Vec::new());
    }

    let mut profiles = Vec::new();

    if let Ok(entries) = fs::read_dir(&main_dir) {
        for entry in entries.flatten() {
            if let Ok(file_type) = entry.file_type() {
                if file_type.is_dir() {
                    if let Some(name) = entry.file_name().to_str() {
                        // Check if this folder has gif_profile.h
                        let gif_header = entry.path().join("gif_profile.h");
                        if gif_header.exists() {
                            profiles.push(name.to_string());
                        }
                    }
                }
            }
        }
    }

    profiles.sort();
    Ok(profiles)
}

/// Changes the active profile by modifying sdkconfig.defaults
pub fn set_active_profile(project_path: &str, profile_name: &str) -> Result<(), ProfileError> {
    let project = Path::new(project_path);

    if !project.exists() {
        return Err(ProfileError::ProjectNotFound);
    }

    // Verify the profile folder exists
    let profile_path = project.join("main").join(profile_name);
    if !profile_path.exists() {
        return Err(ProfileError::ProfileFolderMissing(profile_name.to_string()));
    }

    let sdkconfig = project.join("sdkconfig.defaults");
    if !sdkconfig.exists() {
        return Err(ProfileError::NoSdkConfig);
    }

    let content = fs::read_to_string(&sdkconfig)
        .map_err(|e| ProfileError::WriteError(e.to_string()))?;

    let base = base_of(profile_name);

    // 1. Rewrite the active choice line to the base profile (satisfies the Kconfig choice).
    let choice_re = Regex::new(r"(?m)^\s*CONFIG_GIF_PROFILE_\w+=y").unwrap();
    let base_choice = format!("CONFIG_GIF_PROFILE_{}=y", base.to_uppercase());
    let mut updated = choice_re.replace(&content, base_choice.as_str()).to_string();

    // 2. Upsert the name-override line pointing at the actual profile.
    let name_re = Regex::new(r#"(?m)^\s*CONFIG_GIF_PROFILE_NAME\s*=.*$"#).unwrap();
    let name_line = format!("CONFIG_GIF_PROFILE_NAME=\"{}\"", profile_name);
    if name_re.is_match(&updated) {
        updated = name_re.replace(&updated, name_line.as_str()).to_string();
    } else {
        updated.push('\n');
        updated.push_str(&name_line);
        updated.push('\n');
    }

    fs::write(&sdkconfig, updated)
        .map_err(|e| ProfileError::WriteError(e.to_string()))?;

    // Keep the *generated* sdkconfig in sync too. idf.py build reads the
    // generated sdkconfig, and sdkconfig.defaults alone only takes effect when
    // sdkconfig is first generated — so editing defaults by itself leaves a
    // stale sdkconfig behind and the build keeps compiling the old profile.
    update_sdkconfig(project_path, profile_name)?;

    Ok(())
}

/// Rewrites the generated `sdkconfig` (the file `idf.py build` actually reads)
/// so the active profile choice + name match `profile_name`. A no-op if the
/// generated file doesn't exist yet (it will be created from defaults).
fn update_sdkconfig(project_path: &str, profile_name: &str) -> Result<(), ProfileError> {
    let sdkconfig = Path::new(project_path).join("sdkconfig");
    if !sdkconfig.exists() {
        return Ok(());
    }

    let content = fs::read_to_string(&sdkconfig)
        .map_err(|e| ProfileError::WriteError(e.to_string()))?;

    let base = base_of(profile_name).to_uppercase();
    let symbol_re = Regex::new(r"CONFIG_GIF_PROFILE_([A-Za-z0-9_]+)").unwrap();

    let mut name_written = false;
    let mut out = String::new();

    for line in content.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("CONFIG_GIF_PROFILE_NAME") {
            out.push_str(&format!("CONFIG_GIF_PROFILE_NAME=\"{}\"\n", profile_name));
            name_written = true;
        } else if let Some(cap) = symbol_re.captures(trimmed) {
            let short = cap.get(1).map(|m| m.as_str()).unwrap_or("");
            if short == base {
                out.push_str(&format!("CONFIG_GIF_PROFILE_{}=y\n", base));
            } else {
                out.push_str(&format!("# CONFIG_GIF_PROFILE_{} is not set\n", short));
            }
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }

    if !name_written {
        out.push_str(&format!("CONFIG_GIF_PROFILE_NAME=\"{}\"\n", profile_name));
    }

    fs::write(&sdkconfig, out).map_err(|e| ProfileError::WriteError(e.to_string()))?;
    Ok(())
}

/// Ensures the generated `sdkconfig` matches the active profile named in
/// `sdkconfig.defaults`. `idf.py build` reads the generated sdkconfig (and its
/// value overrides defaults), so this must run before building.
pub fn sync_generated_sdkconfig(project_path: &str) -> Result<(), ProfileError> {
    let active = detect_active_profile(project_path)?;
    update_sdkconfig(project_path, &active.profile)
}

/// Gets the current default emotion from gif_profile.h
pub fn get_default_emotion(project_path: &str) -> Result<String, ProfileError> {
    let profile = detect_active_profile(project_path)?;
    let gif_header = profile.profile_path.join("gif_profile.h");

    let content = fs::read_to_string(&gif_header)
        .map_err(|e| ProfileError::WriteError(e.to_string()))?;

    // Find #define GIF_PROFILE_DEFAULT <emotion>
    let default_re = Regex::new(r"#define\s+GIF_PROFILE_DEFAULT\s+(\w+)").unwrap();
    let default_emotion = default_re.captures(&content)
        .and_then(|cap| cap.get(1))
        .map(|m| m.as_str().to_string())
        .ok_or(ProfileError::MalformedGifTable("Cannot find GIF_PROFILE_DEFAULT".to_string()))?;

    Ok(default_emotion)
}

/// Sets the default emotion in gif_profile.h
pub fn set_default_emotion(project_path: &str, emotion: &str) -> Result<(), ProfileError> {
    let profile = detect_active_profile(project_path)?;

    // Verify the symbol exists in the profile (must be a real lv_img_dsc_t,
    // not a gif_table key like "startup"/"standby").
    if !profile.symbols.contains(&emotion.to_string()) {
        return Err(ProfileError::MalformedGifTable(
            format!("'{}' is not a valid default emotion symbol in this profile", emotion)
        ));
    }

    let gif_header = profile.profile_path.join("gif_profile.h");
    let content = fs::read_to_string(&gif_header)
        .map_err(|e| ProfileError::WriteError(e.to_string()))?;

    // Replace #define GIF_PROFILE_DEFAULT <old> with #define GIF_PROFILE_DEFAULT <new>
    let default_re = Regex::new(r"(#define\s+GIF_PROFILE_DEFAULT\s+)\w+").unwrap();
    let updated_content = default_re.replace(&content, format!("${{1}}{}", emotion));

    fs::write(&gif_header, updated_content.as_ref())
        .map_err(|e| ProfileError::WriteError(e.to_string()))?;

    Ok(())
}

/// Strips a trailing `_<digits>` suffix. "floki_1" -> "floki", "floki" -> "floki".
pub fn base_of(profile: &str) -> String {
    match profile.rfind('_') {
        Some(i) if i + 1 < profile.len()
            && profile[i + 1..].chars().all(|c| c.is_ascii_digit()) =>
        {
            profile[..i].to_string()
        }
        _ => profile.to_string(),
    }
}

/// A profile is a clone iff it has a base distinct from itself.
pub fn is_clone(profile: &str) -> bool {
    base_of(profile) != profile
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

/// Copies a base profile to the next free `<base>_<N>` folder and activates it.
/// Returns the new clone's name.
pub fn create_profile_clone(project_path: &str, base_name: &str) -> Result<String, ProfileError> {
    let project = Path::new(project_path);
    if !project.exists() {
        return Err(ProfileError::ProjectNotFound);
    }

    let base_path = project.join("main").join(base_name);
    if !base_path.exists() {
        return Err(ProfileError::ProfileFolderMissing(base_name.to_string()));
    }

    if is_clone(base_name) {
        return Err(ProfileError::WriteError(format!(
            "'{}' is a clone. Clone a base profile, not another clone.",
            base_name
        )));
    }

    let mut n = 1;
    let new_name = loop {
        let candidate = format!("{}_{}", base_name, n);
        if !project.join("main").join(&candidate).exists() {
            break candidate;
        }
        n += 1;
    };

    let new_path = project.join("main").join(&new_name);
    copy_dir_recursive(&base_path, &new_path)
        .map_err(|e| ProfileError::WriteError(format!("Failed to copy profile: {}", e)))?;

    set_active_profile(project_path, &new_name)?;

    Ok(new_name)
}

/// Sets the given profile's `GIF_PROFILE_DEFAULT` back to its base profile's value.
/// Never touches any gif/*.c file.
pub fn reset_profile_to_default(project_path: &str, profile_name: &str) -> Result<(), ProfileError> {
    let project = Path::new(project_path);
    if !project.exists() {
        return Err(ProfileError::ProjectNotFound);
    }

    let base = base_of(profile_name);

    let base_header = project.join("main").join(&base).join("gif_profile.h");
    let base_content = fs::read_to_string(&base_header)
        .map_err(|_| ProfileError::MalformedGifTable(base_header.to_string_lossy().to_string()))?;
    let default_re = Regex::new(r"#define\s+GIF_PROFILE_DEFAULT\s+(\w+)").unwrap();
    let base_default = default_re.captures(&base_content)
        .and_then(|cap| cap.get(1))
        .map(|m| m.as_str().to_string())
        .ok_or(ProfileError::MalformedGifTable(format!(
            "Cannot find GIF_PROFILE_DEFAULT in {}",
            base
        )))?;

    let profile_header = project.join("main").join(profile_name).join("gif_profile.h");
    let content = fs::read_to_string(&profile_header)
        .map_err(|_| ProfileError::MalformedGifTable(profile_header.to_string_lossy().to_string()))?;
    let replace_re = Regex::new(r"(#define\s+GIF_PROFILE_DEFAULT\s+)\w+").unwrap();
    let updated = replace_re.replace(&content, format!("${{1}}{}", base_default));

    fs::write(&profile_header, updated.as_ref())
        .map_err(|e| ProfileError::WriteError(e.to_string()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_profile_detection() {
        // A synthetic checkout, shaped like a real profile: mixed-case GifEntry,
        // an extern "C" block, a #define default. detect_active_profile reads
        // sdkconfig.defaults (not the generated sdkconfig), so the fixture needs
        // only the name override and the header.
        let dir = std::env::temp_dir().join(format!("gif_tool_profile_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let profile_dir = dir.join("main").join("floki");
        fs::create_dir_all(&profile_dir).unwrap();

        fs::write(
            dir.join("sdkconfig.defaults"),
            "CONFIG_GIF_PROFILE_NAME=\"floki\"\n",
        )
        .unwrap();
        fs::write(
            profile_dir.join("gif_profile.h"),
            r#"
extern "C" {
    extern const lv_img_dsc_t angry;
    extern const lv_img_dsc_t happy;
}

#define GIF_PROFILE_DEFAULT happy

struct GifEntry { const char* name; const lv_img_dsc_t* dsc; };
static const GifEntry gif_table[] = {
    {"angry", &angry},
    {"happy", &happy},
};
"#,
        )
        .unwrap();

        let info = detect_active_profile(dir.to_str().unwrap()).unwrap();
        assert_eq!(info.profile, "floki");
        assert_eq!(info.emotions, vec!["angry", "happy"]);
        assert_eq!(info.symbols, vec!["angry", "happy"]);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_nonexistent_path() {
        let result = detect_active_profile("/path/that/does/not/exist");
        assert!(matches!(result, Err(ProfileError::ProjectNotFound)));
    }

    #[test]
    fn test_base_of() {
        assert_eq!(base_of("floki"), "floki");
        assert_eq!(base_of("floki_1"), "floki");
        assert_eq!(base_of("floki_12"), "floki");
        assert_eq!(base_of("pengu_3"), "pengu");
        // A name with a non-numeric suffix is not treated as a clone.
        assert_eq!(base_of("floki_abc"), "floki_abc");
        assert_eq!(base_of("floki_"), "floki_");
    }

    #[test]
    fn test_is_clone() {
        assert!(!is_clone("floki"));
        assert!(is_clone("floki_1"));
        assert!(is_clone("bduck_42"));
        assert!(!is_clone("floki_abc"));
    }

    fn tmp_project() -> PathBuf {
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let root = std::env::temp_dir().join(format!(
            "gif-tool-test-{}-{}",
            std::process::id(),
            n
        ));
        let _ = fs::remove_dir_all(&root);
        root
    }

    fn write_profile(root: &Path, name: &str, default_emotion: &str, emotions: &[&str]) {
        let dir = root.join("main").join(name);
        fs::create_dir_all(dir.join("gif")).unwrap();
        let mut h = String::from("extern \"C\" {\n");
        for e in emotions {
            h.push_str(&format!("    extern const lv_img_dsc_t {};\n", e));
        }
        h.push_str("}\n\n");
        h.push_str(&format!("#define GIF_PROFILE_DEFAULT {}\n\n", default_emotion));
        h.push_str("struct GifEntry { const char* name; const lv_img_dsc_t* dsc; };\n");
        h.push_str("static const GifEntry gif_table[] = {\n");
        for e in emotions {
            h.push_str(&format!("    {{\"{}\", &{}}},\n", e, e));
        }
        h.push_str("};\n");
        fs::write(dir.join("gif_profile.h"), h).unwrap();
        for e in emotions {
            fs::write(
                dir.join("gif").join(format!("{}.c", e)),
                format!("const lv_img_dsc_t {} = {{}};\n", e),
            )
            .unwrap();
        }
    }

    #[test]
    fn test_detect_name_override() {
        let root = tmp_project();
        write_profile(&root, "floki_1", "happy", &["angry", "happy"]);
        fs::write(
            root.join("sdkconfig.defaults"),
            "CONFIG_GIF_PROFILE_FLOKI=y\nCONFIG_GIF_PROFILE_NAME=\"floki_1\"\n",
        )
        .unwrap();
        let info = detect_active_profile(&root.to_string_lossy()).unwrap();
        assert_eq!(info.profile, "floki_1");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn test_detect_fallback_choice() {
        let root = tmp_project();
        write_profile(&root, "pengu", "neutral", &["neutral", "happy"]);
        fs::write(root.join("sdkconfig.defaults"), "CONFIG_GIF_PROFILE_PENGU=y\n").unwrap();
        let info = detect_active_profile(&root.to_string_lossy()).unwrap();
        assert_eq!(info.profile, "pengu");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn test_symbols_parsed_separately_from_keys() {
        let root = tmp_project();
        let dir = root.join("main").join("floki");
        fs::create_dir_all(dir.join("gif")).unwrap();
        fs::write(
            dir.join("gif_profile.h"),
            "extern \"C\" {\n    extern const lv_img_dsc_t findingWifi;\n    extern const lv_img_dsc_t happy;\n}\n\n#define GIF_PROFILE_DEFAULT findingWifi\n\nstruct GifEntry { const char* name; const lv_img_dsc_t* dsc; };\nstatic const GifEntry gif_table[] = {\n    {\"startup\", &findingWifi},\n    {\"happy\", &happy},\n};\n",
        )
        .unwrap();
        fs::write(root.join("sdkconfig.defaults"), "CONFIG_GIF_PROFILE_FLOKI=y\n").unwrap();

        let info = detect_active_profile(&root.to_string_lossy()).unwrap();
        // Keys come from gif_table[]; symbols come from the extern declarations.
        assert_eq!(info.emotions, vec!["startup", "happy"]);
        assert_eq!(info.symbols, vec!["findingWifi", "happy"]);
        assert!(!info.symbols.contains(&"startup".to_string()));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn test_set_active_profile_writes_name_override() {
        let root = tmp_project();
        write_profile(&root, "floki", "angry", &["angry", "happy"]);
        write_profile(&root, "floki_1", "happy", &["angry", "happy"]);
        fs::write(root.join("sdkconfig.defaults"), "CONFIG_GIF_PROFILE_FLOKI=y\n").unwrap();

        set_active_profile(&root.to_string_lossy(), "floki_1").unwrap();

        let sdk = fs::read_to_string(root.join("sdkconfig.defaults")).unwrap();
        assert!(sdk.contains("CONFIG_GIF_PROFILE_NAME=\"floki_1\""));
        assert!(sdk.contains("CONFIG_GIF_PROFILE_FLOKI=y"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn test_set_active_profile_base() {
        let root = tmp_project();
        write_profile(&root, "floki", "angry", &["angry"]);
        write_profile(&root, "pengu", "neutral", &["neutral"]);
        fs::write(
            root.join("sdkconfig.defaults"),
            "# GIF Profile (uncomment one)\nCONFIG_GIF_PROFILE_FLOKI=y\n# CONFIG_GIF_PROFILE_PENGU=y\n# CONFIG_GIF_PROFILE_BDUCK=y\n",
        )
        .unwrap();

        set_active_profile(&root.to_string_lossy(), "pengu").unwrap();

        let sdk = fs::read_to_string(root.join("sdkconfig.defaults")).unwrap();
        assert!(sdk.contains("CONFIG_GIF_PROFILE_PENGU=y"));
        assert!(sdk.contains("CONFIG_GIF_PROFILE_NAME=\"pengu\""));
        // The commented-out alternatives must NOT be clobbered by the regex.
        assert!(sdk.contains("# CONFIG_GIF_PROFILE_BDUCK=y"));
        assert!(sdk.contains("# CONFIG_GIF_PROFILE_PENGU=y"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn test_set_active_profile_syncs_generated_sdkconfig() {
        let root = tmp_project();
        write_profile(&root, "floki", "angry", &["angry"]);
        write_profile(&root, "floki_1", "happy", &["angry", "happy"]);
        fs::write(
            root.join("sdkconfig.defaults"),
            "CONFIG_GIF_PROFILE_FLOKI=y\nCONFIG_GIF_PROFILE_NAME=\"floki_1\"\n",
        )
        .unwrap();
        // A stale generated sdkconfig still pointing at the base profile.
        fs::write(
            root.join("sdkconfig"),
            "CONFIG_GIF_PROFILE_FLOKI=y\n# CONFIG_GIF_PROFILE_PENGU is not set\n# CONFIG_GIF_PROFILE_BDUCK is not set\nCONFIG_GIF_PROFILE_NAME=\"floki\"\n",
        )
        .unwrap();

        set_active_profile(&root.to_string_lossy(), "floki_1").unwrap();

        let sdk = fs::read_to_string(root.join("sdkconfig")).unwrap();
        assert!(sdk.contains("CONFIG_GIF_PROFILE_NAME=\"floki_1\""));
        assert!(sdk.contains("CONFIG_GIF_PROFILE_FLOKI=y"));
        assert!(sdk.contains("# CONFIG_GIF_PROFILE_PENGU is not set"));
        assert!(sdk.contains("# CONFIG_GIF_PROFILE_BDUCK is not set"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn test_sync_generated_sdkconfig() {
        let root = tmp_project();
        write_profile(&root, "floki", "angry", &["angry"]);
        write_profile(&root, "floki_1", "happy", &["angry", "happy"]);
        fs::write(
            root.join("sdkconfig.defaults"),
            "CONFIG_GIF_PROFILE_FLOKI=y\nCONFIG_GIF_PROFILE_NAME=\"floki_1\"\n",
        )
        .unwrap();
        fs::write(
            root.join("sdkconfig"),
            "CONFIG_GIF_PROFILE_FLOKI=y\n# CONFIG_GIF_PROFILE_PENGU is not set\n# CONFIG_GIF_PROFILE_BDUCK is not set\nCONFIG_GIF_PROFILE_NAME=\"floki\"\n",
        )
        .unwrap();

        // Sync reads the active profile from defaults and rewrites the generated
        // sdkconfig to match.
        sync_generated_sdkconfig(&root.to_string_lossy()).unwrap();

        let sdk = fs::read_to_string(root.join("sdkconfig")).unwrap();
        assert!(sdk.contains("CONFIG_GIF_PROFILE_NAME=\"floki_1\""));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn test_create_profile_clone_naming_and_copy() {
        let root = tmp_project();
        write_profile(&root, "floki", "angry", &["angry", "happy"]);
        fs::write(root.join("sdkconfig.defaults"), "CONFIG_GIF_PROFILE_FLOKI=y\n").unwrap();

        let root_str = root.to_string_lossy().to_string();

        let n1 = create_profile_clone(&root_str, "floki").unwrap();
        let n2 = create_profile_clone(&root_str, "floki").unwrap();
        assert_eq!(n1, "floki_1");
        assert_eq!(n2, "floki_2");

        // Files were copied into each clone.
        assert!(root.join("main/floki_1/gif/angry.c").exists());
        assert!(root.join("main/floki_2/gif/happy.c").exists());

        // The latest clone was activated.
        let sdk = fs::read_to_string(root.join("sdkconfig.defaults")).unwrap();
        assert!(sdk.contains("CONFIG_GIF_PROFILE_NAME=\"floki_2\""));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn test_clone_rejects_clone() {
        let root = tmp_project();
        write_profile(&root, "floki", "angry", &["angry"]);
        write_profile(&root, "floki_1", "angry", &["angry"]);
        fs::write(root.join("sdkconfig.defaults"), "CONFIG_GIF_PROFILE_FLOKI=y\n").unwrap();
        let root_str = root.to_string_lossy().to_string();

        let err = create_profile_clone(&root_str, "floki_1").unwrap_err();
        assert!(err.to_string().contains("not another clone"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn test_reset_profile_to_default() {
        let root = tmp_project();
        write_profile(&root, "floki", "findingWifi", &["findingWifi", "sleepy"]);
        write_profile(&root, "floki_1", "sleepy", &["findingWifi", "sleepy"]);

        // A gif file with sentinel content that must NOT be changed by reset.
        let gif_path = root.join("main/floki_1/gif/sleepy.c");
        let sentinel = "// DO NOT TOUCH\nconst lv_img_dsc_t sleepy = {};\n";
        fs::write(&gif_path, sentinel).unwrap();

        let root_str = root.to_string_lossy().to_string();
        reset_profile_to_default(&root_str, "floki_1").unwrap();

        let header = fs::read_to_string(root.join("main/floki_1/gif_profile.h")).unwrap();
        assert!(header.contains("#define GIF_PROFILE_DEFAULT findingWifi"));

        // GIF file bytes are untouched.
        let after = fs::read_to_string(&gif_path).unwrap();
        assert_eq!(after, sentinel);

        let _ = fs::remove_dir_all(&root);
    }
}
