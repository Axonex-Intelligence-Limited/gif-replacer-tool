// parser.rs - C file parsing and symbol renaming

use regex::Regex;

/// Errors that can occur during C file parsing
#[derive(Debug)]
pub enum ParserError {
    NotLvglImage,
    AmbiguousDeclarations,
    SymbolMismatch,
    FileReadError,
}

impl std::fmt::Display for ParserError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::NotLvglImage => write!(f, "Not a valid LVGL image file. Missing lv_img_dsc_t declaration."),
            Self::AmbiguousDeclarations => write!(f, "Ambiguous: file contains multiple image declarations."),
            Self::SymbolMismatch => write!(f, "Symbol mismatch: descriptor and map array have different names."),
            Self::FileReadError => write!(f, "Cannot read file"),
        }
    }
}

/// Information extracted from an LVGL image file
#[derive(Debug)]
pub struct ImageInfo {
    pub original_name: String,
    pub file_size_bytes: usize,
}

/// Validates that a file is a valid LVGL image
///
/// # Arguments
/// * `file_path` - Path to the .c file to validate
///
/// # Returns
/// * `Ok(ImageInfo)` - File is valid, returns metadata
/// * `Err(ParserError)` - File is invalid or malformed
pub fn validate_lvgl_image(file_path: &str) -> Result<ImageInfo, ParserError> {
    let content = std::fs::read_to_string(file_path)
        .map_err(|_| ParserError::FileReadError)?;

    // Find lv_img_dsc_t declaration
    let desc_re = Regex::new(r"lv_img_dsc_t\s+(\w+)").unwrap();
    let descriptors: Vec<_> = desc_re.captures_iter(&content).collect();

    if descriptors.is_empty() {
        return Err(ParserError::NotLvglImage);
    }

    if descriptors.len() > 1 {
        return Err(ParserError::AmbiguousDeclarations);
    }

    let original_name = descriptors[0].get(1).unwrap().as_str().to_string();

    // Verify matching map array exists
    let map_pattern = format!(r"uint8_t\s+{}_map\[\]", original_name);
    let map_re = Regex::new(&map_pattern).unwrap();

    if !map_re.is_match(&content) {
        return Err(ParserError::SymbolMismatch);
    }

    Ok(ImageInfo {
        original_name,
        file_size_bytes: content.len(),
    })
}

/// Renames symbols in LVGL image file content
///
/// # Arguments
/// * `content` - The file content as a string
/// * `target_emotion` - The new name for the image
///
/// # Returns
/// * `Ok(String)` - Modified content with renamed symbols
/// * `Err(ParserError)` - Content is invalid
pub fn rename_symbols(content: &str, target_emotion: &str) -> Result<String, ParserError> {
    // Validate first
    let info = validate_lvgl_image_content(content)?;
    let old_name = &info.original_name;

    // Replace descriptor: lv_img_dsc_t old_name → lv_img_dsc_t target_emotion
    let desc_pattern = format!(r"lv_img_dsc_t\s+{}\b", old_name);
    let desc_re = Regex::new(&desc_pattern).unwrap();
    let mut result = desc_re.replace(content, format!("lv_img_dsc_t {}", target_emotion)).to_string();

    // Replace map array: old_name_map → target_emotion_map
    let map_pattern = format!(r"\b{}_map\b", old_name);
    let map_re = Regex::new(&map_pattern).unwrap();
    result = map_re.replace_all(&result, format!("{}_map", target_emotion)).to_string();

    Ok(result)
}

// Helper function to validate content without file I/O
fn validate_lvgl_image_content(content: &str) -> Result<ImageInfo, ParserError> {
    let desc_re = Regex::new(r"lv_img_dsc_t\s+(\w+)").unwrap();
    let descriptors: Vec<_> = desc_re.captures_iter(content).collect();

    if descriptors.is_empty() {
        return Err(ParserError::NotLvglImage);
    }

    if descriptors.len() > 1 {
        return Err(ParserError::AmbiguousDeclarations);
    }

    let original_name = descriptors[0].get(1).unwrap().as_str().to_string();

    // Verify matching map array exists
    let map_pattern = format!(r"uint8_t\s+{}_map\[\]", original_name);
    let map_re = Regex::new(&map_pattern).unwrap();

    if !map_re.is_match(content) {
        return Err(ParserError::SymbolMismatch);
    }

    Ok(ImageInfo {
        original_name,
        file_size_bytes: content.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_lvgl_image_content_happy_path() {
        let sample_content = r#"
const uint8_t my_gif_map[] = { 0x00, 0x01, 0x02 };

const lv_img_dsc_t my_gif = {
    .header.always_zero = 0,
    .data = my_gif_map,
};
"#;
        let result = validate_lvgl_image_content(sample_content);
        assert!(result.is_ok());
        let info = result.unwrap();
        assert_eq!(info.original_name, "my_gif");
    }

    #[test]
    fn test_not_lvgl_image() {
        let content = "int main() { return 0; }";
        let result = validate_lvgl_image_content(content);
        assert!(matches!(result, Err(ParserError::NotLvglImage)));
    }

    #[test]
    fn test_multiple_declarations() {
        let content = r#"
const lv_img_dsc_t img1 = {};
const lv_img_dsc_t img2 = {};
"#;
        let result = validate_lvgl_image_content(content);
        assert!(matches!(result, Err(ParserError::AmbiguousDeclarations)));
    }

    #[test]
    fn test_symbol_mismatch() {
        let content = r#"
const uint8_t wrong_name_map[] = { 0x00 };
const lv_img_dsc_t my_gif = {};
"#;
        let result = validate_lvgl_image_content(content);
        assert!(matches!(result, Err(ParserError::SymbolMismatch)));
    }

    #[test]
    fn test_rename_symbols() {
        let sample_content = r#"
const uint8_t my_gif_map[] = { 0x00, 0x01, 0x02 };

const lv_img_dsc_t my_gif = {
    .header.always_zero = 0,
    .data = my_gif_map,
};
"#;

        let result = rename_symbols(sample_content, "angry").unwrap();
        assert!(result.contains("angry_map"));
        assert!(result.contains("lv_img_dsc_t angry"));
        assert!(!result.contains("my_gif_map"));
        assert!(!result.contains("lv_img_dsc_t my_gif"));
    }

    #[test]
    fn test_rename_symbols_multiple_references() {
        let sample_content = r#"
const uint8_t test_map[] = { 0x00 };
const lv_img_dsc_t test = {
    .data = test_map,
    .data_size = sizeof(test_map),
};
"#;

        let result = rename_symbols(sample_content, "happy").unwrap();
        assert!(result.contains("happy_map"));
        assert!(result.contains("lv_img_dsc_t happy"));
        // Verify all occurrences are replaced (including inside sizeof())
        assert_eq!(result.matches("happy_map").count(), 3);
    }
}
