// gifconv.rs - GIF pre-flight validation and gif2c.py invocation

use std::fmt;
use std::path::{Path, PathBuf};

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
    PythonMissing,
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
            Self::PythonMissing => write!(
                f,
                "python3 not found. Install Apple's developer tools, or drop a \
                 pre-made .c file instead."
            ),
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
}
