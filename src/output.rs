use crate::files;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub fn generate_output(
    project_name: &str,
    tree: &str,
    files_to_include: &[PathBuf],
    root_path: &Path,
) -> Result<String> {
    let mut output = String::new();

    output.push_str("<repo-to-text>\n");
    output.push_str(&format!("Directory: {}\n\n", project_name));
    output.push_str("Directory Structure:\n");
    output.push_str("<directory_structure>\n");
    output.push_str(tree);
    output.push_str("</directory_structure>\n");

    for file_path in files_to_include {
        let relative_path = file_path.strip_prefix(root_path)?;
        output.push_str(&format!(
            "\n<content full_path=\"{}\">\n",
            relative_path.to_string_lossy()
        ));

        let content = files::read_file_contents(file_path)
            .with_context(|| format!("Failed to read content of file: {:?}", file_path))?;
        output.push_str(&content);
        output.push_str("\n</content>\n");
    }

    output.push_str("\n</repo-to-text>\n");

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Helper function to create a temporary file with content
    fn create_test_file(dir: &Path, name: &str, content: &str) -> PathBuf {
        let file_path = dir.join(name);
        fs::write(&file_path, content).unwrap();
        file_path
    }

    #[test]
    fn test_output_contains_all_required_sections() {
        let temp_dir = TempDir::new().unwrap();
        let root_path = temp_dir.path();

        let file_path = create_test_file(root_path, "test.txt", "content");

        let output = generate_output(
            "my_project",
            "any tree structure",
            &[file_path],
            root_path
        ).unwrap();

        assert!(output.starts_with("<repo-to-text>\n"));
        assert!(output.contains("Directory: my_project\n"));
        assert!(output.contains("Directory Structure:\n"));
        assert!(output.contains("<directory_structure>\n"));
        assert!(output.contains("</directory_structure>\n"));
        assert!(output.ends_with("</repo-to-text>\n"));
    }

    #[test]
    fn test_tree_string_is_included_verbatim() {
        let temp_dir = TempDir::new().unwrap();
        let custom_tree = "my custom tree format\nwith multiple lines\n";

        let output = generate_output(
            "project",
            custom_tree,
            &[],
            temp_dir.path()
        ).unwrap();

        assert!(output.contains(custom_tree));
    }

    #[test]
    fn test_single_file_content_is_included() {
        let temp_dir = TempDir::new().unwrap();
        let root_path = temp_dir.path();

        let file_content = "fn main() {\n    println!(\"Hello\");\n}";
        let file_path = create_test_file(root_path, "main.rs", file_content);

        let output = generate_output(
            "project",
            "tree",
            &[file_path],
            root_path
        ).unwrap();

        assert!(output.contains("<content full_path=\"main.rs\">"));
        assert!(output.contains(file_content));
        assert!(output.contains("\n</content>\n"));
    }

    #[test]
    fn test_multiple_files_in_order() {
        let temp_dir = TempDir::new().unwrap();
        let root_path = temp_dir.path();

        let file1 = create_test_file(root_path, "a.txt", "First");
        let file2 = create_test_file(root_path, "b.txt", "Second");
        let file3 = create_test_file(root_path, "c.txt", "Third");

        let output = generate_output(
            "project",
            "tree",
            &[file1, file2, file3],
            root_path
        ).unwrap();

        let pos_a = output.find("full_path=\"a.txt\"").unwrap();
        let pos_b = output.find("full_path=\"b.txt\"").unwrap();
        let pos_c = output.find("full_path=\"c.txt\"").unwrap();

        assert!(pos_a < pos_b && pos_b < pos_c);
        assert!(output.contains("First"));
        assert!(output.contains("Second"));
        assert!(output.contains("Third"));
    }

    #[test]
    fn test_nested_paths_use_relative_paths() {
        let temp_dir = TempDir::new().unwrap();
        let root_path = temp_dir.path();

        let subdir = root_path.join("src").join("models");
        fs::create_dir_all(&subdir).unwrap();

        let file_path = create_test_file(&subdir, "user.rs", "struct User {}");

        let output = generate_output(
            "project",
            "tree",
            &[file_path],
            root_path
        ).unwrap();

        let has_unix_path = output.contains("full_path=\"src/models/user.rs\"");
        let has_windows_path = output.contains("full_path=\"src\\models\\user.rs\"");
        assert!(has_unix_path || has_windows_path);
    }

    #[test]
    fn test_empty_file_list() {
        let temp_dir = TempDir::new().unwrap();

        let output = generate_output(
            "empty_project",
            "empty tree",
            &[],
            temp_dir.path()
        ).unwrap();

        assert!(output.contains("<repo-to-text>"));
        assert!(output.contains("Directory: empty_project"));
        assert!(!output.contains("<content"));
    }

    #[test]
    fn test_empty_file_content() {
        let temp_dir = TempDir::new().unwrap();
        let root_path = temp_dir.path();

        let file_path = create_test_file(root_path, "empty.txt", "");

        let output = generate_output(
            "project",
            "tree",
            &[file_path],
            root_path
        ).unwrap();

        assert!(output.contains("<content full_path=\"empty.txt\">"));
        assert!(output.contains("</content>"));
    }

    #[test]
    fn test_unicode_content() {
        let temp_dir = TempDir::new().unwrap();
        let root_path = temp_dir.path();

        let unicode = "Hello 世界 🦀 Здравствуй мир";
        let file_path = create_test_file(root_path, "unicode.txt", unicode);

        let output = generate_output(
            "project",
            "tree",
            &[file_path],
            root_path
        ).unwrap();

        assert!(output.contains(unicode));
    }

    #[test]
    fn test_special_xml_characters_not_escaped() {
        let temp_dir = TempDir::new().unwrap();
        let root_path = temp_dir.path();

        let content = "<tag> & \"quotes\" </tag>";
        let file_path = create_test_file(root_path, "special.txt", content);

        let output = generate_output(
            "project",
            "tree",
            &[file_path],
            root_path
        ).unwrap();

        assert!(output.contains(content));
    }

    #[test]
    fn test_filename_with_spaces() {
        let temp_dir = TempDir::new().unwrap();
        let root_path = temp_dir.path();

        let file_path = create_test_file(root_path, "my file.txt", "content");

        let output = generate_output(
            "project",
            "tree",
            &[file_path],
            root_path
        ).unwrap();

        assert!(output.contains("full_path=\"my file.txt\""));
    }

    #[test]
    fn test_nonexistent_file_returns_error() {
        let temp_dir = TempDir::new().unwrap();
        let root_path = temp_dir.path();
        let nonexistent = root_path.join("missing.txt");

        let result = generate_output(
            "project",
            "tree",
            &[nonexistent],
            root_path
        );

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Failed to read content of file"));
    }

    #[test]
    fn test_file_outside_root_returns_error() {
        let temp_dir1 = TempDir::new().unwrap();
        let temp_dir2 = TempDir::new().unwrap();

        let file_in_dir2 = create_test_file(temp_dir2.path(), "file.txt", "content");

        let result = generate_output(
            "project",
            "tree",
            &[file_in_dir2],
            temp_dir1.path()
        );
        assert!(result.is_err());
    }
}