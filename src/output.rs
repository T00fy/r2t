use anyhow::{Result};
use serde::Serialize;
use std::fmt::Write;

#[derive(Serialize, Debug)]
pub struct FileContent {
    pub full_path: String,
    pub content: String,
}

#[derive(Serialize, Debug)]
#[serde(rename = "repo-to-text")]
pub struct RepoRepresentation {
    pub directory: String,
    pub directory_structure: String,
    #[serde(rename = "content")]
    pub contents: Vec<FileContent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Yaml,
    Json,
    Xml,
}

pub fn render(format: OutputFormat, repo: &RepoRepresentation) -> Result<String> {
    match format {
        OutputFormat::Yaml => serialize_yaml(repo),
        OutputFormat::Json => render_json_like_readable(repo),
        OutputFormat::Xml => render_pseudo_xml(repo),
    }
}

fn serialize_yaml(repo: &RepoRepresentation) -> Result<String> {
    let mut output = String::new();

    writeln!(output, "directory: {}", yaml_escape_simple(&repo.directory))?;

    write_yaml_literal(&mut output, "directory_structure", &repo.directory_structure, 0)?;

    writeln!(output, "content:")?;
    for file in &repo.contents {
        writeln!(output, "  - full_path: {}", yaml_escape_simple(&file.full_path))?;
        write_yaml_literal(&mut output, "content", &file.content, 2)?;
    }

    Ok(output)
}

/// Writes a field as a YAML literal block (|) with proper indentation
fn write_yaml_literal(output: &mut String, field_name: &str, content: &str, indent_level: usize) -> Result<()> {
    let indent = "  ".repeat(indent_level);
    let content_indent = "  ".repeat(indent_level + 1);

    writeln!(output, "{}{}: |", indent, field_name)?;
    for line in content.lines() {
        writeln!(output, "{}{}", content_indent, line)?;
    }
    if !content.is_empty() && !content.ends_with('\n') {
        writeln!(output, "{}", content_indent)?;
    }

    Ok(())
}

/// Escapes a string for YAML if needed (for simple strings without newlines)
fn yaml_escape_simple(s: &str) -> String {
    if s.is_empty() || s.contains(['\n', ':', '#', '\'', '"', '|', '>', '[', ']', '{', '}'])
        || s.starts_with([' ', '-', '?', '@', '`'])
        || s.ends_with(' ')
    {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        s.to_string()
    }
}

/// Renders a highly readable, but not strictly valid, JSON-like format.
/// It uses triple-quotes for multi-line strings.
fn render_json_like_readable(repo: &RepoRepresentation) -> Result<String> {
    let mut output = String::new();

    writeln!(output, "{{")?;
    writeln!(output, "  \"directory\": {},", serde_json::to_string(&repo.directory)?)?;

    writeln!(output, "  \"directory_structure\": \"\"\"")?;
    write!(output, "{}", repo.directory_structure)?;
    if !repo.directory_structure.is_empty() && !repo.directory_structure.ends_with('\n') {
        writeln!(output)?;
    }
    writeln!(output, "\"\"\",")?;

    writeln!(output, "  \"content\": [")?;
    let num_files = repo.contents.len();
    for (i, file) in repo.contents.iter().enumerate() {
        writeln!(output, "    {{")?;
        writeln!(output, "      \"full_path\": {},", serde_json::to_string(&file.full_path)?)?;
        writeln!(output, "      \"content\": \"\"\"")?;
        write!(output, "{}", file.content)?;
        if !file.content.is_empty() && !file.content.ends_with('\n') {
            writeln!(output)?;
        }
        writeln!(output, "\"\"\"")?;

        if i < num_files - 1 {
            writeln!(output, "    }},")?;
        } else {
            writeln!(output, "    }}")?;
        }
    }
    writeln!(output, "  ]")?;
    writeln!(output, "}}")?;

    Ok(output)
}

/// Renders the output using the original pseudo-XML format.
fn render_pseudo_xml(repo: &RepoRepresentation) -> Result<String> {
    let mut output = String::new();

    // Write header
    writeln!(output, "<repo-to-text>")?;
    writeln!(output, "Directory: {}", repo.directory)?;
    writeln!(output, "\nDirectory Structure:")?;
    writeln!(output, "<directory_structure>")?;
    write!(output, "{}", &repo.directory_structure)?;
    if !repo.directory_structure.ends_with('\n') {
        writeln!(output)?;
    }
    writeln!(output, "</directory_structure>")?;

    for file in &repo.contents {
        writeln!(
            output,
            "\n<content full_path=\"{}\">",
            file.full_path
        )?;
        write!(output, "{}", file.content)?;
        if !file.content.is_empty() && !file.content.ends_with('\n') {
            writeln!(output)?;
        }
        writeln!(output, "</content>")?;
    }

    writeln!(output, "\n</repo-to-text>")?;

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_repo() -> RepoRepresentation {
        RepoRepresentation {
            directory: "my_project".to_string(),
            directory_structure: ".
└── main.rs"
                .to_string(),
            contents: vec![
                FileContent {
                    full_path: "main.rs".to_string(),
                    content: "fn main() {\n    println!(\"Hello, world!\");\n}".to_string(),
                },
                FileContent {
                    full_path: "empty.txt".to_string(),
                    content: "".to_string(),
                },
            ],
        }
    }

    #[test]
    fn test_render_yaml() {
        let repo = create_test_repo();
        let output = render(OutputFormat::Yaml, &repo).unwrap();
        println!("YAML output:\n{}", output);
        assert!(output.contains("directory: my_project"));
        assert!(output.contains("full_path: main.rs"));
        assert!(output.contains("content: |"));
    }

    #[test]
    fn test_render_pseudo_json_readable() {
        let repo = create_test_repo();
        let output = render(OutputFormat::Json, &repo).unwrap();

        assert!(output.contains(r#""directory": "my_project""#));
        // Check for triple-quote-delimited blocks
        assert!(output.contains(r#""directory_structure": """
.
└── main.rs
""","#));
        assert!(output.contains(r#""full_path": "main.rs""#));
        assert!(output.contains(r#""content": """
fn main() {
    println!("Hello, world!");
}
""""#));
    }

    #[test]
    fn test_render_pseudo_xml() {
        let repo = create_test_repo();
        let output = render(OutputFormat::Xml, &repo).unwrap();

        assert!(output.contains("<repo-to-text>"));
        assert!(output.contains("Directory: my_project"));
        assert!(output.contains("<directory_structure>"));
        assert!(output.contains(".
└── main.rs"));
        assert!(output.contains("</directory_structure>"));
        assert!(output.contains("<content full_path=\"main.rs\">"));
        assert!(output.contains("println!(\"Hello, world!\");"));
        assert!(output.contains("</content>"));
        assert!(output.contains("<content full_path=\"empty.txt\">"));
    }
}