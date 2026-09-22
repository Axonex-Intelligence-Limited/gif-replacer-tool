// gifconv.rs - GIF pre-flight validation and gif2c.py invocation

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::builder::BuildResult;
use crate::profile::ProfileInfo;

/// The EmotionDisplay panel is 800x480 (main/lvgl_port.h:24-25).
/// A GIF is embedded at its native size, so anything else renders wrong.
pub const EXPECTED_SIZE: (u16, u16) = (800, 480);

#[derive(Debug)]
pub enum GifConvError {
    ReadError(String),
    TooShort(usize),
    NotAGif([u8; 3]),
    WrongSize { found: (u16, u16), expected: (u16, u16) },
    Gif2cMissing(PathBuf),
    PythonMissing(Vec<String>),
    ConversionFailed { exit_code: i32, output: String },
}

impl fmt::Display for GifConvError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::ReadError(msg) => write!(f, "Cannot read the file: {}", msg),
            Self::TooShort(len) => write!(
                f,
                "File is too short to be a GIF ({} bytes). It may be empty or truncated.",
                len
            ),
            Self::NotAGif(b) => match b {
                [0x89, b'P', b'N'] => write!(
                    f,
                    "Not a GIF file — this looks like a PNG. Drop the original .gif."
                ),
                [0xff, 0xd8, 0xff] => write!(
                    f,
                    "Not a GIF file — this looks like a JPG. Drop the original .gif."
                ),
                _ => write!(f, "Not a GIF file. Drop the original .gif (GIF87a or GIF89a)."),
            },
            Self::WrongSize { found, expected } => write!(
                f,
                "This GIF is {}x{}, but the display is {}x{}. \
                 A GIF is embedded at its native size, so a smaller one renders \
                 in a corner of the screen. Resize to {}x{} and drop it again.",
                found.0, found.1, expected.0, expected.1, expected.0, expected.1
            ),
            Self::Gif2cMissing(path) => write!(
                f,
                "Your EmotionDisplay checkout has no {} . Pull the latest, \
                 or drop a pre-made .c file instead.",
                path.display()
            ),
            Self::PythonMissing(tried) => {
                let mut msg = String::from("No Python interpreter found for gif2c.py.\n\nTried:");
                for t in tried {
                    msg.push_str(&format!("\n  {}", t));
                }
                msg.push_str("\n\nInstall Python 3, or drop a pre-made .c file instead.");
                write!(f, "{}", msg)
            },
            Self::ConversionFailed { exit_code, output } => write!(
                f,
                "gif2c.py failed (exit {}):\n{}",
                exit_code, output
            ),
        }
    }
}

#[derive(Debug)]
pub struct GifProbe {
    pub version: String,
    pub width: u16,
    pub height: u16,
    pub bytes: u64,
}

/// Reads the 10-byte GIF header. Not a parser — a header sniff for pre-flight.
///
/// Width and height live at bytes 6..8 and 8..10 as little-endian u16, which is
/// enough to reject a wrong-sized file before spawning python3.
pub fn probe_gif(path: &Path) -> Result<GifProbe, GifConvError> {
    let data = std::fs::read(path).map_err(|e| GifConvError::ReadError(e.to_string()))?;

    if data.len() < 10 {
        return Err(GifConvError::TooShort(data.len()));
    }
    if &data[0..3] != b"GIF" {
        return Err(GifConvError::NotAGif([data[0], data[1], data[2]]));
    }

    let width = u16::from_le_bytes([data[6], data[7]]);
    let height = u16::from_le_bytes([data[8], data[9]]);

    if (width, height) != EXPECTED_SIZE {
        return Err(GifConvError::WrongSize { found: (width, height), expected: EXPECTED_SIZE });
    }

    Ok(GifProbe {
        version: String::from_utf8_lossy(&data[3..6]).to_string(),
        width,
        height,
        bytes: data.len() as u64,
    })
}

/// Locates the converter script inside an EmotionDisplay checkout. A missing
/// script means the checkout predates commit fc2de1a.
pub fn find_gif2c(project: &Path) -> Result<PathBuf, GifConvError> {
    let script = project.join("tools").join("gif2c.py");
    if script.is_file() {
        Ok(script)
    } else {
        Err(GifConvError::Gif2cMissing(script))
    }
}

/// Converts `gif` into `<profile>/gif/<emotion>.c` by invoking the project's own
/// gif2c.py, so GCT repair and the self-check stay in one place.
///
/// gif2c.py writes its output only after its self-check passes, so a rejection
/// here leaves the existing .c file untouched.
/// An interpreter plus any leading args it needs — `py -3` is two tokens.
#[derive(Debug, Clone)]
pub struct PythonCmd {
    pub program: String,
    pub args: Vec<String>,
}

/// How to name this interpreter in the "Tried:" list.
fn describe_python(cmd: &PythonCmd) -> String {
    if cmd.args.is_empty() {
        cmd.program.clone()
    } else {
        format!("{} {}", cmd.program, cmd.args.join(" "))
    }
}

/// Interpreters to try for gif2c.py, most likely first.
///
/// The IDF venv leads: the build layer already located a usable ESP-IDF, and
/// its python env lives inside IDF_TOOLS_PATH, so it is the one interpreter we
/// can be sure exists. `py -3` and `python` follow so conversion still works on
/// a machine with no ESP-IDF at all — `gif2c.py` is stdlib-only, so any 3.7+
/// will do.
#[cfg(windows)]
fn python_candidates(idf_tools_path: Option<&Path>) -> Vec<PythonCmd> {
    let mut v = Vec::new();

    if let Some(tools) = idf_tools_path {
        // <tools>\python_env\idf5.5_py3.11_env\Scripts\python.exe — the env
        // directory name carries both the IDF and python versions, so match
        // whatever is there rather than spelling out a name that drifts on the
        // next minor bump.
        if let Ok(entries) = std::fs::read_dir(tools.join("python_env")) {
            for e in entries.flatten() {
                let exe = e.path().join("Scripts").join("python.exe");
                if exe.is_file() {
                    v.push(PythonCmd {
                        program: exe.to_string_lossy().to_string(),
                        args: Vec::new(),
                    });
                }
            }
        }
    }

    v.push(PythonCmd { program: "py".into(), args: vec!["-3".into()] });
    v.push(PythonCmd { program: "python".into(), args: Vec::new() });
    v
}

/// Unix is untouched: `python3` is present and correct there.
#[cfg(not(windows))]
fn python_candidates(_idf_tools_path: Option<&Path>) -> Vec<PythonCmd> {
    vec![PythonCmd { program: "python3".into(), args: Vec::new() }]
}

pub fn run_gif2c_sync(
    project: &Path,
    profile: &ProfileInfo,
    gif: &Path,
    emotion: &str,
) -> Result<BuildResult, GifConvError> {
    let script = find_gif2c(project)?;
    let out_path = profile.profile_path.join("gif").join(format!("{}.c", emotion));

    let tools = crate::builder::resolved_idf_tools_path();
    let candidates = python_candidates(tools.as_deref());

    let mut tried: Vec<String> = Vec::new();
    let mut success: Option<std::process::Output> = None;

    for candidate in &candidates {
        tried.push(describe_python(candidate));

        let mut cmd = Command::new(&candidate.program);
        cmd.args(&candidate.args)
            .arg(&script)
            .arg(gif)
            .arg("--name")
            .arg(emotion)
            .arg("-o")
            .arg(&out_path);

        match cmd.output() {
            // A spawned process that exits non-zero is still a found
            // interpreter — the failure below reports it properly.
            Ok(output) => {
                success = Some(output);
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(GifConvError::ReadError(e.to_string())),
        }
    }

    let Some(output) = success else {
        return Err(GifConvError::PythonMissing(tried));
    };

    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    if !output.status.success() {
        return Err(GifConvError::ConversionFailed {
            exit_code: output.status.code().unwrap_or(-1),
            output: combined,
        });
    }

    Ok(BuildResult {
        success: true,
        exit_code: 0,
        output: combined,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;

    /// A GIF header is 10 bytes: "GIF8?a" + w(2 LE) + h(2 LE).
    fn gif_header(version: &[u8; 6], w: u16, h: u16) -> Vec<u8> {
        let mut v = version.to_vec();
        v.extend_from_slice(&w.to_le_bytes());
        v.extend_from_slice(&h.to_le_bytes());
        v
    }

    fn write_temp(name: &str, bytes: &[u8]) -> PathBuf {
        let p = std::env::temp_dir().join(format!("gifconv_test_{}_{}", std::process::id(), name));
        let mut f = std::fs::File::create(&p).unwrap();
        f.write_all(bytes).unwrap();
        p
    }

    #[test]
    fn probe_accepts_gif89a_800x480() {
        let p = write_temp("a.gif", &gif_header(b"GIF89a", 800, 480));
        let probe = probe_gif(&p).unwrap();
        assert_eq!(probe.version, "89a");
        assert_eq!(probe.width, 800);
        assert_eq!(probe.height, 480);
        assert_eq!(probe.bytes, 10);
    }

    #[test]
    fn probe_accepts_gif87a_800x480() {
        let p = write_temp("b.gif", &gif_header(b"GIF87a", 800, 480));
        assert!(probe_gif(&p).is_ok());
    }

    #[test]
    fn probe_rejects_wrong_size() {
        let p = write_temp("c.gif", &gif_header(b"GIF89a", 400, 240));
        match probe_gif(&p) {
            Err(GifConvError::WrongSize { found, expected }) => {
                assert_eq!(found, (400, 240));
                assert_eq!(expected, (800, 480));
            }
            other => panic!("expected WrongSize, got {:?}", other),
        }
    }

    #[test]
    fn probe_rejects_short_file() {
        let p = write_temp("d.gif", b"GIF");
        assert!(matches!(probe_gif(&p), Err(GifConvError::TooShort(3))));
    }

    #[test]
    fn probe_rejects_png() {
        let p = write_temp("e.gif", &[0x89, b'P', b'N', b'G', 0, 0, 0, 0, 0, 0]);
        assert!(matches!(probe_gif(&p), Err(GifConvError::NotAGif(_))));
    }

    #[test]
    fn probe_reports_missing_file() {
        let p = std::env::temp_dir().join("gifconv_test_definitely_absent.gif");
        let _ = std::fs::remove_file(&p);
        assert!(matches!(probe_gif(&p), Err(GifConvError::ReadError(_))));
    }

    #[test]
    fn find_gif2c_reports_missing_script() {
        let dir = std::env::temp_dir().join(format!("gifconv_noscript_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        match find_gif2c(&dir) {
            Err(GifConvError::Gif2cMissing(p)) => assert!(p.ends_with("tools/gif2c.py")),
            other => panic!("expected Gif2cMissing, got {:?}", other),
        }
    }

    #[test]
    fn find_gif2c_locates_script_when_present() {
        let dir = std::env::temp_dir().join(format!("gifconv_script_{}", std::process::id()));
        std::fs::create_dir_all(dir.join("tools")).unwrap();
        std::fs::write(dir.join("tools").join("gif2c.py"), b"# stub").unwrap();
        assert_eq!(find_gif2c(&dir).unwrap(), dir.join("tools").join("gif2c.py"));
    }

    /// Integration check against a real EmotionDisplay checkout. Ignored by
    /// default: it needs the sibling repo, writes into it, and depends on
    /// python3. This is the only coverage of the subprocess path.
    ///
    ///     EMOTIONDISPLAY_PATH=/path/to/EmotionDisplay \
    ///       cargo test run_gif2c_against_real_project -- --ignored --nocapture
    ///
    /// It OVERWRITES <profile>/gif/angry.c. Restore it afterwards with:
    ///     git -C $EMOTIONDISPLAY_PATH checkout main/<profile>/gif/angry.c
    #[test]
    #[ignore]
    fn run_gif2c_against_real_project() {
        let project = std::path::PathBuf::from(
            std::env::var("EMOTIONDISPLAY_PATH")
                .expect("set EMOTIONDISPLAY_PATH to an EmotionDisplay checkout"),
        );

        let profile = crate::profile::detect_active_profile(project.to_str().unwrap()).unwrap();

        let gif = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("gif")
            .join("angry_800x480.gif");
        assert!(gif.is_file(), "fixture missing at {}", gif.display());

        let result = run_gif2c_sync(&project, &profile, &gif, "angry").unwrap();
        println!("--- gif2c.py output ---\n{}", result.output);

        assert!(result.output.contains("MISSING -> promoted"), "GCT repair did not run");
        assert!(result.output.contains("self-check"), "self-check did not run");

        let written = profile.profile_path.join("gif").join("angry.c");
        assert!(written.is_file(), "no .c written to {}", written.display());
    }

    #[test]
    fn test_python_missing_message_names_every_interpreter() {
        let err = GifConvError::PythonMissing(vec![
            "C:\\Espressif\\python_env\\idf5.5_py3.11_env\\Scripts\\python.exe".to_string(),
            "py -3".to_string(),
            "python".to_string(),
        ]);
        let msg = err.to_string();

        assert!(msg.contains("idf5.5_py3.11_env"));
        assert!(msg.contains("py -3"));
        assert!(msg.contains("python"));
        assert!(
            msg.contains("pre-made .c file"),
            "the message must offer the .c escape hatch: {}",
            msg
        );
        assert!(
            !msg.contains("Apple's developer tools"),
            "that hint is macOS-only and must not survive: {}",
            msg
        );
    }

    #[test]
    fn test_describe_python_joins_leading_args() {
        assert_eq!(
            describe_python(&PythonCmd { program: "python".to_string(), args: vec![] }),
            "python"
        );
        assert_eq!(
            describe_python(&PythonCmd { program: "py".to_string(), args: vec!["-3".to_string()] }),
            "py -3"
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn test_python_candidates_unix_is_exactly_python3() {
        // The non-goals promise Unix behaviour is unchanged.
        let candidates = python_candidates(None);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].program, "python3");
        assert!(candidates[0].args.is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn test_python_candidates_prefer_the_idf_env() {
        let root = std::env::temp_dir().join(format!("gif_tool_py_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let scripts = root
            .join("python_env")
            .join("idf5.5_py3.11_env")
            .join("Scripts");
        std::fs::create_dir_all(&scripts).unwrap();
        std::fs::write(scripts.join("python.exe"), b"stub").unwrap();

        let candidates = python_candidates(Some(&root));

        assert!(
            candidates[0].program.ends_with("python.exe"),
            "the IDF env must lead: {:?}",
            candidates
        );
        assert!(candidates[0].program.contains("idf5.5_py3.11_env"));
        assert_eq!(candidates[1].program, "py");
        assert_eq!(candidates[1].args, vec!["-3".to_string()]);
        assert_eq!(candidates[2].program, "python");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(windows)]
    #[test]
    fn test_python_candidates_without_idf_start_at_py() {
        // A machine with Python but no ESP-IDF must still convert.
        let candidates = python_candidates(None);
        assert_eq!(candidates[0].program, "py");
        assert_eq!(candidates[1].program, "python");
    }
}
