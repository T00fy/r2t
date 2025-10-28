use anyhow::Result;
use std::fs;
use std::path::Path;

const BINARY_CHECK_BYTES: usize = 8192; // Check first 8KB for null bytes

/// Checks if a file is a binary type that should be excluded.
/// Allows text-based files and SVGs.
pub fn is_binary_or_image(path: &Path) -> Result<bool> {
    if !path.is_file() {
        return Ok(false);
    }

    if let Some(kind) = infer::get_from_path(path)? {
        let mime = kind.mime_type();
        return Ok(!mime.starts_with("text/") && mime != "image/svg+xml");
    }

    let bytes = fs::read(path)?;
    Ok(bytes[..bytes.len().min(BINARY_CHECK_BYTES)].contains(&0))
}

/// Reads file content into a string, using lossy conversion for non-UTF8 files.
pub fn read_file_contents(path: &Path) -> Result<String> {
    let bytes = fs::read(path)?;

    // Try efficient conversion first, fall back to lossy if needed
    String::from_utf8(bytes)
        .or_else(|e| Ok(String::from_utf8_lossy(e.as_bytes()).into_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_file(dir: &TempDir, name: &str, content: &[u8]) -> std::path::PathBuf {
        let file_path = dir.path().join(name);
        fs::write(&file_path, content).unwrap();
        file_path
    }

    #[test]
    fn test_is_binary_or_image_with_text_file() {
        let temp_dir = TempDir::new().unwrap();
        let text_file = create_test_file(&temp_dir, "test.txt", b"Hello, World!");

        let result = is_binary_or_image(&text_file).unwrap();
        assert!(!result, "Text files should not be classified as binary");
    }

    #[test]
    fn test_is_binary_or_image_with_markdown_file() {
        let temp_dir = TempDir::new().unwrap();
        let md_file = create_test_file(&temp_dir, "test.md", b"# Markdown\nSome content");

        let result = is_binary_or_image(&md_file).unwrap();
        assert!(!result, "Markdown files should not be classified as binary");
    }

    #[test]
    fn test_is_binary_or_image_with_svg() {
        let temp_dir = TempDir::new().unwrap();
        let svg_content = br#"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100">
  <circle cx="50" cy="50" r="40" fill="red" />
</svg>"#;
        let svg_file = create_test_file(&temp_dir, "test.svg", svg_content);

        let result = is_binary_or_image(&svg_file).unwrap();
        assert!(!result, "SVG files should not be classified as binary");
    }

    #[test]
    fn test_is_binary_or_image_with_png() {
        let temp_dir = TempDir::new().unwrap();
        // Minimal valid PNG header
        let png_bytes = vec![
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG signature
            0x00, 0x00, 0x00, 0x0D, // IHDR chunk length
            0x49, 0x48, 0x44, 0x52, // IHDR
            0x00, 0x00, 0x00, 0x01, // Width: 1
            0x00, 0x00, 0x00, 0x01, // Height: 1
            0x08, 0x02, 0x00, 0x00, 0x00, // Bit depth, color type, etc.
        ];
        let png_file = create_test_file(&temp_dir, "test.png", &png_bytes);

        let result = is_binary_or_image(&png_file).unwrap();
        assert!(result, "PNG files should be classified as binary");
    }

    #[test]
    fn test_is_binary_or_image_with_jpeg() {
        let temp_dir = TempDir::new().unwrap();
        // JPEG signature
        let jpeg_bytes = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46];
        let jpeg_file = create_test_file(&temp_dir, "test.jpg", &jpeg_bytes);

        let result = is_binary_or_image(&jpeg_file).unwrap();
        assert!(result, "JPEG files should be classified as binary");
    }

    #[test]
    fn test_is_binary_or_image_with_pdf() {
        let temp_dir = TempDir::new().unwrap();
        // PDF signature
        let pdf_bytes = b"%PDF-1.4\n";
        let pdf_file = create_test_file(&temp_dir, "test.pdf", pdf_bytes);

        let result = is_binary_or_image(&pdf_file).unwrap();
        assert!(result, "PDF files should be classified as binary");
    }

    #[test]
    fn test_is_binary_or_image_with_zip() {
        let temp_dir = TempDir::new().unwrap();
        // ZIP signature
        let zip_bytes = vec![0x50, 0x4B, 0x03, 0x04];
        let zip_file = create_test_file(&temp_dir, "test.zip", &zip_bytes);

        let result = is_binary_or_image(&zip_file).unwrap();
        assert!(result, "ZIP files should be classified as binary");
    }

    #[test]
    fn test_is_binary_or_image_with_unknown_text_file() {
        let temp_dir = TempDir::new().unwrap();
        let text_file = create_test_file(&temp_dir, "unknown.xyz", b"Plain text without magic bytes");

        let result = is_binary_or_image(&text_file).unwrap();
        assert!(!result, "UTF-8 text files without magic bytes should not be classified as binary");
    }

    #[test]
    fn test_is_binary_or_image_with_unknown_binary_file() {
        let temp_dir = TempDir::new().unwrap();
        // Random binary data that's not valid UTF-8
        let binary_bytes = vec![0xFF, 0xFE, 0x00, 0x01, 0x80, 0xFF];
        let binary_file = create_test_file(&temp_dir, "unknown.bin", &binary_bytes);

        let result = is_binary_or_image(&binary_file).unwrap();
        assert!(result, "Non-UTF-8 files without recognizable magic bytes should be classified as binary");
    }

    #[test]
    fn test_is_binary_or_image_with_empty_file() {
        let temp_dir = TempDir::new().unwrap();
        let empty_file = create_test_file(&temp_dir, "empty.txt", b"");

        let result = is_binary_or_image(&empty_file).unwrap();
        assert!(!result, "Empty files should not be classified as binary");
    }

    #[test]
    fn test_is_binary_or_image_with_directory() {
        let temp_dir = TempDir::new().unwrap();
        let dir_path = temp_dir.path().join("subdir");
        fs::create_dir(&dir_path).unwrap();

        let result = is_binary_or_image(&dir_path).unwrap();
        assert!(!result, "Directories should return false");
    }

    #[test]
    fn test_is_binary_or_image_with_nonexistent_file() {
        let temp_dir = TempDir::new().unwrap();
        let nonexistent = temp_dir.path().join("does_not_exist.txt");

        let result = is_binary_or_image(&nonexistent).unwrap();
        assert!(!result, "Non-existent paths should return false");
    }

    #[test]
    fn test_is_binary_or_image_with_utf8_json() {
        let temp_dir = TempDir::new().unwrap();
        let json_file = create_test_file(&temp_dir, "test.json", br#"{"key": "value"}"#);

        let result = is_binary_or_image(&json_file).unwrap();
        assert!(!result, "JSON files should not be classified as binary");
    }

    #[test]
    fn test_is_binary_or_image_with_html() {
        let temp_dir = TempDir::new().unwrap();
        let html_content = b"<!DOCTYPE html><html><body><h1>Test</h1></body></html>";
        let html_file = create_test_file(&temp_dir, "test.html", html_content);

        let result = is_binary_or_image(&html_file).unwrap();
        assert!(!result, "HTML files should not be classified as binary");
    }

    #[test]
    fn test_read_file_contents_with_valid_utf8() {
        let temp_dir = TempDir::new().unwrap();
        let content = "Hello, World! 你好世界";
        let text_file = create_test_file(&temp_dir, "test.txt", content.as_bytes());

        let result = read_file_contents(&text_file).unwrap();
        assert_eq!(result, content);
    }

    #[test]
    fn test_read_file_contents_with_invalid_utf8() {
        let temp_dir = TempDir::new().unwrap();
        // Invalid UTF-8 sequence
        let invalid_bytes = vec![0x48, 0x65, 0x6C, 0x6C, 0x6F, 0xFF, 0xFE];
        let binary_file = create_test_file(&temp_dir, "test.bin", &invalid_bytes);

        let result = read_file_contents(&binary_file).unwrap();
        // Should use lossy conversion
        assert!(result.contains("Hello"));
        assert!(result.contains('\u{FFFD}')); // Replacement character
    }

    #[test]
    fn test_read_file_contents_with_empty_file() {
        let temp_dir = TempDir::new().unwrap();
        let empty_file = create_test_file(&temp_dir, "empty.txt", b"");

        let result = read_file_contents(&empty_file).unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_read_file_contents_with_nonexistent_file() {
        let temp_dir = TempDir::new().unwrap();
        let nonexistent = temp_dir.path().join("does_not_exist.txt");

        let result = read_file_contents(&nonexistent);
        assert!(result.is_err(), "Reading non-existent file should return an error");
    }

    #[test]
    fn test_read_file_contents_with_multiline() {
        let temp_dir = TempDir::new().unwrap();
        let content = "Line 1\nLine 2\nLine 3";
        let text_file = create_test_file(&temp_dir, "multiline.txt", content.as_bytes());

        let result = read_file_contents(&text_file).unwrap();
        assert_eq!(result, content);
        assert_eq!(result.lines().count(), 3);
    }

    #[test]
    fn test_read_file_contents_with_special_characters() {
        let temp_dir = TempDir::new().unwrap();
        let content = "Special: \t\r\n\0 chars";
        let text_file = create_test_file(&temp_dir, "special.txt", content.as_bytes());

        let result = read_file_contents(&text_file).unwrap();
        assert_eq!(result, content);
    }

    #[test]
    fn test_read_file_contents_with_large_file() {
        let temp_dir = TempDir::new().unwrap();
        let content = "x".repeat(10000);
        let large_file = create_test_file(&temp_dir, "large.txt", content.as_bytes());

        let result = read_file_contents(&large_file).unwrap();
        assert_eq!(result.len(), 10000);
        assert_eq!(result, content);
    }

    #[test]
    fn test_is_binary_or_image_with_executable() {
        let temp_dir = TempDir::new().unwrap();
        // ELF header with invalid UTF-8 bytes to ensure it's detected as binary
        // Adding some high bytes that are invalid UTF-8
        let elf_bytes = vec![
            0x7F, 0x45, 0x4C, 0x46, // ELF magic
            0x02, 0x01, 0x01, 0x00,
            0xFF, 0xFE, 0xFD, 0xFC  // Invalid UTF-8 bytes
        ];
        let exe_file = create_test_file(&temp_dir, "test.elf", &elf_bytes);

        let result = is_binary_or_image(&exe_file).unwrap();
        assert!(result, "Executable files should be classified as binary");
    }

    #[test]
    fn test_is_binary_or_image_with_csv() {
        let temp_dir = TempDir::new().unwrap();
        let csv_content = b"name,age,city\nJohn,30,NYC\nJane,25,LA";
        let csv_file = create_test_file(&temp_dir, "test.csv", csv_content);

        let result = is_binary_or_image(&csv_file).unwrap();
        assert!(!result, "CSV files should not be classified as binary");
    }
}