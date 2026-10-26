// stock.rs - saved originals and reset-to-original for one GIF slot
//
// The app overwrites <profile>/gif/<emotion>.c in place, so a slot can only be
// reset if an original is kept somewhere else. Two sources exist:
//
//   1. A backup taken before the tool first overwrites a slot. Works for every
//      profile, including a base profile that has no second copy.
//   2. For a clone (<base>_<N>), the base profile's own file. This rescues
//      slots changed before backups existed, and clones only exist because the
//      base does.
//
// Backups live outside the project, under the per-user app data directory, so
// they never appear in the repository and never reach the firmware build.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::profile::{base_of, is_clone, ProfileInfo};

const APP_DIR: &str = "GifReplacerTool";
const STOCK_SUBDIR: &str = "stock";
const ABSENT_MARKER: &str = "absent\n";
const MAX_NAME_CHARS: usize = 40;

static TEMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug)]
pub enum StockError {
    NoOriginal { profile: String, emotion: String },
    UnsafeName(String),
    CacheUnavailable,
    Io(String),
}

impl fmt::Display for StockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoOriginal { profile, emotion } => write!(
                f,
                "No original copy is available for '{}' in profile '{}'. \
                 The app only keeps originals it saved before a slot was changed.",
                emotion, profile
            ),
            Self::UnsafeName(name) => write!(
                f,
                "Refusing to use an unsafe profile or emotion name: '{}'.",
                name
            ),
            Self::CacheUnavailable => write!(
                f,
                "Cannot locate the app data folder used to store original GIFs."
            ),
            Self::Io(message) => write!(f, "Could not access the saved original: {}", message),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum RestoreOutcome {
    FromBackup,
    FromBase,
    RemovedCreatedFile,
}

// ---------------------------------------------------------------- public API

pub fn ensure_backup(project: &Path, profile: &ProfileInfo, emotion: &str) -> Result<(), StockError> {
    let root = cache_root().ok_or(StockError::CacheUnavailable)?;
    ensure_backup_in(&root, project, profile, emotion)
}

pub fn restore(
    project: &Path,
    profile: &ProfileInfo,
    emotion: &str,
) -> Result<RestoreOutcome, StockError> {
    let root = cache_root().ok_or(StockError::CacheUnavailable)?;
    restore_in(&root, project, profile, emotion)
}

/// The profile's emotions whose slot currently differs from a restorable
/// original. Never fails: a profile load must not break because saved originals
/// are unavailable.
pub fn available_emotions(project: &Path, profile: &ProfileInfo) -> Vec<String> {
    match cache_root() {
        Some(root) => available_emotions_in(&root, project, profile),
        None => Vec::new(),
    }
}

// ------------------------------------------------------------- cache layout

fn cache_root() -> Option<PathBuf> {
    dirs::data_local_dir().map(|dir| dir.join(APP_DIR).join(STOCK_SUBDIR))
}

/// A stable, filesystem-safe key for a project. The canonical path makes two
/// checkouts at different locations independent; the FNV hash keeps it short.
fn project_key(project: &Path) -> String {
    let canonical = fs::canonicalize(project).unwrap_or_else(|_| project.to_path_buf());
    let hash = fnv1a_hex(canonical.to_string_lossy().as_bytes());

    let folder = canonical
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();

    let sanitized: String = folder
        .chars()
        .take(MAX_NAME_CHARS)
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' })
        .collect();

    let name = if sanitized.trim_matches('_').is_empty() {
        "project"
    } else {
        sanitized.as_str()
    };

    format!("prj-{}-{}", name, hash)
}

fn fnv1a_hex(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{:016x}", hash)
}

fn key_dir(root: &Path, project: &Path) -> PathBuf {
    root.join(project_key(project))
}

fn backup_path(key_dir: &Path, profile: &str, emotion: &str) -> PathBuf {
    key_dir.join(profile).join(format!("{}.c", emotion))
}

fn absent_path(key_dir: &Path, profile: &str, emotion: &str) -> PathBuf {
    key_dir.join(profile).join(format!("{}.{}", emotion, ABSENT_MARKER.trim()))
}

fn target_path(profile: &ProfileInfo, emotion: &str) -> PathBuf {
    profile.profile_path.join("gif").join(format!("{}.c", emotion))
}

// ------------------------------------------------------------------- safety

fn safe_component(value: &str) -> bool {
    if value.is_empty() {
        return false;
    }
    if !value.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return false;
    }
    !is_reserved_device_name(value)
}

/// Windows reserves these names even with an extension ("CON.c" is invalid).
fn is_reserved_device_name(value: &str) -> bool {
    let upper = value.to_ascii_uppercase();
    if matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL") {
        return true;
    }
    let bytes = upper.as_bytes();
    bytes.len() == 4
        && (upper.starts_with("COM") || upper.starts_with("LPT"))
        && bytes[3].is_ascii_digit()
}

fn validate_names(profile: &str, emotion: &str) -> Result<(), StockError> {
    if !safe_component(profile) {
        return Err(StockError::UnsafeName(profile.to_string()));
    }
    if !safe_component(emotion) {
        return Err(StockError::UnsafeName(emotion.to_string()));
    }
    Ok(())
}

// --------------------------------------------------------- filesystem utils

fn io_err(error: io::Error) -> StockError {
    StockError::Io(error.to_string())
}

fn non_empty_file(path: &Path) -> bool {
    fs::metadata(path)
        .map(|meta| meta.is_file() && meta.len() > 0)
        .unwrap_or(false)
}

fn remove_if_exists(path: &Path) -> Result<(), StockError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_err(error)),
    }
}

fn temp_path_for(destination: &Path) -> PathBuf {
    let name = destination
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".to_string());
    let sequence = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    destination.with_file_name(format!(
        ".{}.tmp-{}-{}",
        name,
        std::process::id(),
        sequence
    ))
}

/// Copy through a temporary file in the destination directory and rename, so a
/// killed process can never leave a truncated backup or slot that would later
/// be treated as the original.
fn copy_atomic(source: &Path, destination: &Path) -> io::Result<()> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = temp_path_for(destination);
    let result = fs::copy(source, &temporary).and_then(|_| fs::rename(&temporary, destination));
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn write_atomic(destination: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = temp_path_for(destination);
    let result = fs::write(&temporary, bytes).and_then(|_| fs::rename(&temporary, destination));
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn files_differ(target: &Path, source: &Path) -> bool {
    let (target_meta, source_meta) = match (fs::metadata(target), fs::metadata(source)) {
        (Ok(a), Ok(b)) => (a, b),
        _ => return true,
    };

    if target_meta.len() != source_meta.len() {
        return true;
    }

    match (fs::read(target), fs::read(source)) {
        (Ok(a), Ok(b)) => a != b,
        _ => true,
    }
}

// ------------------------------------------------------------- resolution

enum Source {
    /// This profile's own saved original.
    Backup(PathBuf),
    /// This profile's own record that the slot had no file.
    Absent,
    /// The base profile's saved original (clones only).
    BaseBackup(PathBuf),
    /// The base profile's record that the slot had no file (clones only).
    BaseAbsent,
    /// The base profile's current file (clones only).
    BaseFile(PathBuf),
}

/// The base profile directory, but only when this really is a clone: the name
/// has a numeric suffix AND `main/<base>/gif_profile.h` exists. Without the
/// header check, a legitimate base profile called `robot_2` would be treated as
/// a clone of `robot`.
fn base_profile_dir(profile: &ProfileInfo) -> Option<PathBuf> {
    if !is_clone(&profile.profile) {
        return None;
    }

    let base = base_of(&profile.profile);
    if !safe_component(&base) {
        return None;
    }

    let main_dir = profile.profile_path.parent()?;
    let directory = main_dir.join(&base);
    if directory.join("gif_profile.h").is_file() {
        Some(directory)
    } else {
        None
    }
}

fn resolve_source(key_dir: &Path, profile: &ProfileInfo, emotion: &str) -> Option<Source> {
    let own_backup = backup_path(key_dir, &profile.profile, emotion);
    if non_empty_file(&own_backup) {
        return Some(Source::Backup(own_backup));
    }

    if absent_path(key_dir, &profile.profile, emotion).is_file() {
        return Some(Source::Absent);
    }

    let base_dir = base_profile_dir(profile)?;
    let base_name = base_of(&profile.profile);

    let base_backup = backup_path(key_dir, &base_name, emotion);
    if non_empty_file(&base_backup) {
        return Some(Source::BaseBackup(base_backup));
    }

    if absent_path(key_dir, &base_name, emotion).is_file() {
        return Some(Source::BaseAbsent);
    }

    let base_file = base_dir.join("gif").join(format!("{}.c", emotion));
    if base_file.is_file() {
        return Some(Source::BaseFile(base_file));
    }

    None
}

// --------------------------------------------------------------- operations

fn ensure_backup_in(
    root: &Path,
    project: &Path,
    profile: &ProfileInfo,
    emotion: &str,
) -> Result<(), StockError> {
    validate_names(&profile.profile, emotion)?;

    let key_dir = key_dir(root, project);
    let backup = backup_path(&key_dir, &profile.profile, emotion);
    let absent = absent_path(&key_dir, &profile.profile, emotion);

    // First wins. A zero-byte backup is treated as a failed earlier attempt and
    // replaced, because restoring an empty .c would break the build.
    if non_empty_file(&backup) || absent.is_file() {
        return Ok(());
    }

    let target = target_path(profile, emotion);
    if target.is_file() {
        copy_atomic(&target, &backup).map_err(io_err)
    } else {
        write_atomic(&absent, ABSENT_MARKER.as_bytes()).map_err(io_err)
    }
}

fn seed_own_backup(key_dir: &Path, profile: &ProfileInfo, emotion: &str, source: &Path) {
    let destination = backup_path(key_dir, &profile.profile, emotion);
    if !non_empty_file(&destination) {
        let _ = copy_atomic(source, &destination);
    }
}

fn restore_in(
    root: &Path,
    project: &Path,
    profile: &ProfileInfo,
    emotion: &str,
) -> Result<RestoreOutcome, StockError> {
    validate_names(&profile.profile, emotion)?;

    let key_dir = key_dir(root, project);
    let target = target_path(profile, emotion);

    let source = resolve_source(&key_dir, profile, emotion).ok_or_else(|| StockError::NoOriginal {
        profile: profile.profile.clone(),
        emotion: emotion.to_string(),
    })?;

    match source {
        Source::Backup(path) => {
            copy_atomic(&path, &target).map_err(io_err)?;
            Ok(RestoreOutcome::FromBackup)
        }
        Source::Absent => {
            remove_if_exists(&target)?;
            Ok(RestoreOutcome::RemovedCreatedFile)
        }
        Source::BaseBackup(path) => {
            copy_atomic(&path, &target).map_err(io_err)?;
            seed_own_backup(&key_dir, profile, emotion, &path);
            Ok(RestoreOutcome::FromBase)
        }
        Source::BaseFile(path) => {
            copy_atomic(&path, &target).map_err(io_err)?;
            seed_own_backup(&key_dir, profile, emotion, &path);
            Ok(RestoreOutcome::FromBase)
        }
        Source::BaseAbsent => {
            remove_if_exists(&target)?;
            let own_marker = absent_path(&key_dir, &profile.profile, emotion);
            let _ = write_atomic(&own_marker, ABSENT_MARKER.as_bytes());
            Ok(RestoreOutcome::RemovedCreatedFile)
        }
    }
}

fn available_emotions_in(root: &Path, project: &Path, profile: &ProfileInfo) -> Vec<String> {
    if !safe_component(&profile.profile) {
        return Vec::new();
    }

    let key_dir = key_dir(root, project);
    let mut result = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for emotion in &profile.emotions {
        if !seen.insert(emotion.clone()) {
            continue;
        }
        if !safe_component(emotion) {
            continue;
        }

        let target = target_path(profile, emotion);
        let has_work = match resolve_source(&key_dir, profile, emotion) {
            Some(Source::Backup(source))
            | Some(Source::BaseBackup(source))
            | Some(Source::BaseFile(source)) => files_differ(&target, &source),
            Some(Source::Absent) | Some(Source::BaseAbsent) => target.is_file(),
            None => false,
        };

        if has_work {
            result.push(emotion.clone());
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "gif_stock_{}_{}",
            std::process::id(),
            name
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn write_file(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut file = fs::File::create(path).unwrap();
        file.write_all(contents.as_bytes()).unwrap();
    }

    fn read_file(path: &Path) -> String {
        fs::read_to_string(path).unwrap()
    }

    fn profile_at(project: &Path, name: &str, emotions: &[&str]) -> ProfileInfo {
        let profile_path = project.join("main").join(name);
        fs::create_dir_all(profile_path.join("gif")).unwrap();
        write_file(&profile_path.join("gif_profile.h"), "// fixture\n");
        ProfileInfo {
            profile: name.to_string(),
            emotions: emotions.iter().map(|value| value.to_string()).collect(),
            symbols: Vec::new(),
            profile_path,
        }
    }

    fn slot(profile: &ProfileInfo, emotion: &str) -> PathBuf {
        profile.profile_path.join("gif").join(format!("{}.c", emotion))
    }

    #[test]
    fn test_ensure_backup_copies_existing_slot_first_wins() {
        let project = temp_dir("backup_first_wins");
        let root = temp_dir("backup_first_wins_cache");
        let profile = profile_at(&project, "pengu", &["crying"]);

        write_file(&slot(&profile, "crying"), "original");
        ensure_backup_in(&root, &project, &profile, "crying").unwrap();

        let backup = backup_path(&key_dir(&root, &project), "pengu", "crying");
        assert_eq!(read_file(&backup), "original");

        write_file(&slot(&profile, "crying"), "changed");
        ensure_backup_in(&root, &project, &profile, "crying").unwrap();
        assert_eq!(
            read_file(&backup),
            "original",
            "a second backup must not replace the first"
        );
    }

    #[test]
    fn test_ensure_backup_records_absent_slot() {
        let project = temp_dir("backup_absent");
        let root = temp_dir("backup_absent_cache");
        let profile = profile_at(&project, "pengu", &["startup"]);

        ensure_backup_in(&root, &project, &profile, "startup").unwrap();

        let marker = absent_path(&key_dir(&root, &project), "pengu", "startup");
        assert!(marker.is_file(), "missing slot must leave an absent marker");
    }

    #[test]
    fn test_ensure_backup_replaces_zero_byte_backup() {
        let project = temp_dir("backup_zero");
        let root = temp_dir("backup_zero_cache");
        let profile = profile_at(&project, "pengu", &["crying"]);
        write_file(&slot(&profile, "crying"), "original");

        let backup = backup_path(&key_dir(&root, &project), "pengu", "crying");
        fs::create_dir_all(backup.parent().unwrap()).unwrap();
        fs::write(&backup, b"").unwrap();

        ensure_backup_in(&root, &project, &profile, "crying").unwrap();
        assert_eq!(read_file(&backup), "original");
    }

    #[test]
    fn test_restore_prefers_own_backup_over_base() {
        let project = temp_dir("restore_own_backup");
        let root = temp_dir("restore_own_backup_cache");
        let base = profile_at(&project, "pengu", &["crying"]);
        let clone = profile_at(&project, "pengu_1", &["crying"]);

        write_file(&slot(&base, "crying"), "base");
        write_file(&slot(&clone, "crying"), "changed");

        let backup = backup_path(&key_dir(&root, &project), "pengu_1", "crying");
        write_file(&backup, "own-original");

        let outcome = restore_in(&root, &project, &clone, "crying").unwrap();
        assert_eq!(outcome, RestoreOutcome::FromBackup);
        assert_eq!(read_file(&slot(&clone, "crying")), "own-original");
    }

    #[test]
    fn test_restore_clone_falls_back_to_base_and_seeds_backup() {
        let project = temp_dir("restore_base_fallback");
        let root = temp_dir("restore_base_fallback_cache");
        let base = profile_at(&project, "pengu", &["crying"]);
        let clone = profile_at(&project, "pengu_1", &["crying"]);

        write_file(&slot(&base, "crying"), "base-original");
        write_file(&slot(&clone, "crying"), "changed");

        let outcome = restore_in(&root, &project, &clone, "crying").unwrap();
        assert_eq!(outcome, RestoreOutcome::FromBase);
        assert_eq!(read_file(&slot(&clone, "crying")), "base-original");

        let seeded = backup_path(&key_dir(&root, &project), "pengu_1", "crying");
        assert_eq!(read_file(&seeded), "base-original");
    }

    #[test]
    fn test_restore_clone_prefers_base_backup_over_base_current_file() {
        let project = temp_dir("restore_base_backup");
        let root = temp_dir("restore_base_backup_cache");
        let base = profile_at(&project, "pengu", &["crying"]);
        let clone = profile_at(&project, "pengu_1", &["crying"]);

        write_file(&slot(&base, "crying"), "base-edited");
        write_file(&slot(&clone, "crying"), "changed");

        let base_backup = backup_path(&key_dir(&root, &project), "pengu", "crying");
        write_file(&base_backup, "base-original");

        let outcome = restore_in(&root, &project, &clone, "crying").unwrap();
        assert_eq!(outcome, RestoreOutcome::FromBase);
        assert_eq!(
            read_file(&slot(&clone, "crying")),
            "base-original",
            "the base's saved original must win over its edited current file"
        );
    }

    #[test]
    fn test_restore_clone_requires_base_profile_header() {
        let project = temp_dir("restore_requires_header");
        let root = temp_dir("restore_requires_header_cache");
        let clone = profile_at(&project, "pengu_2", &["crying"]);
        write_file(&slot(&clone, "crying"), "changed");

        // main/pengu exists but has no gif_profile.h: not a real base profile.
        write_file(&project.join("main").join("pengu").join("gif").join("crying.c"), "base");

        let result = restore_in(&root, &project, &clone, "crying");
        assert!(matches!(result, Err(StockError::NoOriginal { .. })));
        assert_eq!(read_file(&slot(&clone, "crying")), "changed");
    }

    #[test]
    fn test_restore_removes_file_created_after_absent_marker() {
        let project = temp_dir("restore_absent");
        let root = temp_dir("restore_absent_cache");
        let profile = profile_at(&project, "pengu", &["startup"]);

        let marker = absent_path(&key_dir(&root, &project), "pengu", "startup");
        write_file(&marker, ABSENT_MARKER);

        write_file(&slot(&profile, "startup"), "tool-created");
        let outcome = restore_in(&root, &project, &profile, "startup").unwrap();

        assert_eq!(outcome, RestoreOutcome::RemovedCreatedFile);
        assert!(!slot(&profile, "startup").exists());
    }

    #[test]
    fn test_restore_base_without_backup_reports_no_original_and_leaves_target() {
        let project = temp_dir("restore_base_none");
        let root = temp_dir("restore_base_none_cache");
        let profile = profile_at(&project, "pengu", &["crying"]);
        write_file(&slot(&profile, "crying"), "changed");

        let result = restore_in(&root, &project, &profile, "crying");
        assert!(matches!(result, Err(StockError::NoOriginal { .. })));
        assert_eq!(read_file(&slot(&profile, "crying")), "changed");
    }

    #[test]
    fn test_restore_rejects_unsafe_component() {
        let project = temp_dir("restore_unsafe");
        let root = temp_dir("restore_unsafe_cache");
        let profile = profile_at(&project, "pengu", &["crying"]);

        let result = restore_in(&root, &project, &profile, "../crying");
        assert!(matches!(result, Err(StockError::UnsafeName(_))));

        let reserved = restore_in(&root, &project, &profile, "CON");
        assert!(matches!(reserved, Err(StockError::UnsafeName(_))));
    }

    #[test]
    fn test_available_emotions_filters_dedupes_and_preserves_order() {
        let project = temp_dir("available");
        let root = temp_dir("available_cache");
        let profile = profile_at(&project, "pengu", &["b", "a", "b", "same", "none"]);

        let key_directory = key_dir(&root, &project);
        write_file(&backup_path(&key_directory, "pengu", "a"), "original-a");
        write_file(&slot(&profile, "a"), "changed-a");

        write_file(&backup_path(&key_directory, "pengu", "same"), "same");
        write_file(&slot(&profile, "same"), "same");

        write_file(&slot(&profile, "b"), "changed-b");
        write_file(&slot(&profile, "none"), "changed-none");

        let available = available_emotions_in(&root, &project, &profile);
        assert_eq!(available, vec!["a".to_string()]);
    }

    #[test]
    fn test_restore_is_idempotent_and_leaves_no_temp_files() {
        let project = temp_dir("restore_idempotent");
        let root = temp_dir("restore_idempotent_cache");
        let base = profile_at(&project, "pengu", &["crying"]);
        let clone = profile_at(&project, "pengu_1", &["crying"]);

        write_file(&slot(&base, "crying"), "base-original");
        write_file(&slot(&clone, "crying"), "changed");

        restore_in(&root, &project, &clone, "crying").unwrap();
        let second = restore_in(&root, &project, &clone, "crying").unwrap();

        assert_eq!(second, RestoreOutcome::FromBackup);
        assert_eq!(read_file(&slot(&clone, "crying")), "base-original");

        let leftovers: Vec<String> = fs::read_dir(clone.profile_path.join("gif"))
            .unwrap()
            .flatten()
            .filter_map(|entry| entry.file_name().to_str().map(|name| name.to_string()))
            .filter(|name| name.starts_with('.'))
            .collect();
        assert!(leftovers.is_empty(), "temporary files left behind: {:?}", leftovers);
    }

    #[test]
    fn test_project_key_is_stable_and_path_specific() {
        let first = temp_dir("key_first");
        let second = temp_dir("key_second");

        let a = project_key(&first);
        let b = project_key(&first);
        let c = project_key(&second);

        assert_eq!(a, b);
        assert_ne!(a, c);
        assert!(a.starts_with("prj-"));
    }

    #[test]
    fn test_project_key_sanitizes_invalid_reserved_names() {
        // A space and '!' are legal on Windows and exercise the sanitizer;
        // characters Windows forbids ('\\', '/', ':') cannot be created here.
        let directory = temp_dir("key_sanitize").join("a b!c");
        fs::create_dir_all(&directory).unwrap();

        let key = project_key(&directory);
        assert!(key.starts_with("prj-"));
        assert!(!key.contains(' '), "key must not contain spaces: {}", key);
        assert!(!key.contains('!'), "key must not contain punctuation: {}", key);
    }
}
