use assert_cmd::prelude::*;
use predicates::prelude::*;
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::tempdir;

type TestResult = Result<(), Box<dyn std::error::Error>>;
const MAIN_RS_CONTENT: &str = "fn main() {}";
const README_CONTENT: &str = "This is a test.";
const GITIGNORE_CONTENT: &str = "*.log";

/// Creates a new r2t command with the given project root
fn r2t_cmd(project_root: &Path) -> Result<Command, Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("r2t")?;
    cmd.arg(project_root.to_str().unwrap()).arg("--stdout");
    Ok(cmd)
}

/// Sets up a basic Rust project structure
fn setup_basic_project(root: &Path) -> TestResult {
    fs::create_dir(root.join("src"))?;
    fs::write(root.join("src/main.rs"), MAIN_RS_CONTENT)?;
    fs::write(root.join("README.md"), README_CONTENT)?;
    Ok(())
}

/// Creates a .gitignore file with the given patterns
fn create_gitignore(root: &Path, patterns: &str) -> TestResult {
    fs::write(root.join(".gitignore"), patterns)?;
    Ok(())
}

/// Creates a .r2t.yaml config file with the given content
fn create_r2t_config(root: &Path, config: &str) -> TestResult {
    fs::write(root.join(".r2t.yaml"), config)?;
    Ok(())
}

/// Builds a predicate that checks for all strings in the slice
fn contains_all(strings: &[&str]) -> impl Predicate<str> {
    strings
        .iter()
        .fold(predicate::always().boxed(), |acc, &s| {
            acc.and(predicate::str::contains(s)).boxed()
        })
}

/// Builds a predicate that checks none of the strings are present
fn contains_none(strings: &[&str]) -> impl Predicate<str> {
    strings
        .iter()
        .fold(predicate::always().boxed(), |acc, &s| {
            acc.and(predicate::str::contains(s).not()).boxed()
        })
}

struct FormatExpectations {
    yaml: Vec<&'static str>,
    json: Vec<&'static str>,
    xml: Vec<&'static str>,
}

impl FormatExpectations {
    fn new() -> Self {
        Self {
            yaml: Vec::new(),
            json: Vec::new(),
            xml: Vec::new(),
        }
    }

    fn yaml(mut self, checks: &[&'static str]) -> Self {
        self.yaml.extend_from_slice(checks);
        self
    }

    fn json(mut self, checks: &[&'static str]) -> Self {
        self.json.extend_from_slice(checks);
        self
    }

    fn xml(mut self, checks: &[&'static str]) -> Self {
        self.xml.extend_from_slice(checks);
        self
    }
}

/// Tests a setup function across all three output formats
fn test_all_formats<F>(setup: F, expectations: FormatExpectations) -> TestResult
where
    F: Fn(&Path) -> TestResult,
{
    for (format, checks) in [
        ("yaml", &expectations.yaml),
        ("json", &expectations.json),
        ("xml", &expectations.xml),
    ] {
        let dir = tempdir()?;
        setup(dir.path())?;

        r2t_cmd(dir.path())?
            .arg("--format")
            .arg(format)
            .assert()
            .success()
            .stdout(contains_all(checks));
    }
    Ok(())
}

#[test]
fn test_cli_basic_run_default_yaml() -> TestResult {
    let dir = tempdir()?;
    let root = dir.path();

    setup_basic_project(root)?;
    create_gitignore(root, GITIGNORE_CONTENT)?;
    fs::write(root.join("output.log"), "some log data")?;

    r2t_cmd(root)?.assert().success().stdout(
        contains_all(&[
            "<repo-to-text>",
            "Directory:",
            "Directory Structure:",
            "<directory_structure>",
            "<content full_path=\"README.md\">",
            "This is a test.",
            "</content>",
            "<content full_path=\"src/main.rs\">",
            "fn main() {}",
        ])
            .and(predicate::str::is_match(r"(?m)^\s*[├└]─ .gitignore$").unwrap())
            .and(contains_none(&["output.log"])),
    );

    Ok(())
}

#[test]
fn test_cli_no_gitignore_flag() -> TestResult {
    let dir = tempdir()?;
    let root = dir.path();

    fs::write(root.join("not_ignored.txt"), "I am here")?;
    fs::write(root.join("ignored.txt"), "I should be ignored")?;
    create_gitignore(root, "ignored.txt\n")?;

    r2t_cmd(root)?
        .assert()
        .success()
        .stdout(contains_all(&["not_ignored.txt"]).and(contains_none(&["I should be ignored"])));

    r2t_cmd(root)?
        .arg("--no-gitignore")
        .assert()
        .success()
        .stdout(contains_all(&[
            "<content full_path=\"ignored.txt\">",
            "I should be ignored",
            "<content full_path=\"not_ignored.txt\">",
            "I am here",
        ]));

    Ok(())
}

#[test]
fn test_r2t_yaml_config_ignores() -> TestResult {
    let dir = tempdir()?;
    let root = dir.path();

    setup_basic_project(root)?;
    fs::create_dir(root.join("docs"))?;
    fs::write(root.join("docs/guide.md"), "A guide.")?;

    create_r2t_config(
        root,
        r#"
ignore-tree-and-content:
  - "docs/"
ignore-content:
  - "README.md"
"#,
    )?;

    r2t_cmd(root)?.assert().success().stdout(
        contains_all(&[
            "<content full_path=\"src/main.rs\">",
            "fn main() {}",
        ])
            .and(predicate::str::is_match(r"(?m)^\s*[├└]─ README.md$").unwrap())
            .and(contains_none(&[
                "docs/",
                "guide.md",
                "full_path=\"README.md\"",
                "This is a test.",
            ])),
    );

    Ok(())
}

#[test]
fn test_binary_file_exclusion() -> TestResult {
    let dir = tempdir()?;
    let root = dir.path();

    let png_data = include_bytes!("fixtures/test.png");
    fs::write(root.join("logo.png"), png_data)?;
    fs::write(root.join("icon.svg"), "<svg></svg>")?;

    r2t_cmd(root)?.assert().success().stdout(
        contains_all(&[
            "<content full_path=\"icon.svg\">",
            "<svg></svg>",
        ])
            .and(predicate::str::contains("logo.png").not()),
    );

    Ok(())
}

#[test]
fn test_skip_tests_go_and_java() -> TestResult {
    let dir = tempdir()?;
    let root = dir.path();

    // Setup Go files
    fs::write(root.join("main.go"), "package main")?;
    fs::write(root.join("main_test.go"), "package main_test")?;

    // Setup Java directory structure
    let java_paths = [
        ("src/main/java/App.java", "public class App {}"),
        ("src/test/java/AppTest.java", "public class AppTest {}"),
    ];

    for (path, content) in &java_paths {
        let full_path = root.join(path);
        fs::create_dir_all(full_path.parent().unwrap())?;
        fs::write(full_path, content)?;
    }

    // Without --skip-tests: all files should be present
    r2t_cmd(root)?.assert().success().stdout(contains_all(&[
        "<content full_path=\"main_test.go\">",
        "package main_test",
        "<content full_path=\"src/test/java/AppTest.java\">",
        "public class AppTest {}",
    ]));

    // With --skip-tests: test files appear in tree but not in content
    r2t_cmd(root)?
        .arg("--skip-tests")
        .assert()
        .success()
        .stdout(
            contains_all(&[
                "<content full_path=\"main.go\">",
                "package main",
                "<content full_path=\"src/main/java/App.java\">",
                "public class App {}",
                "main_test.go",
                "AppTest.java",
            ])
                .and(contains_none(&[
                    "full_path=\"main_test.go\"",
                    "package main_test",
                    "full_path=\"src/test/java/AppTest.java\"",
                    "public class AppTest {}",
                ])),
        );

    Ok(())
}

#[test]
fn test_skip_tests_rust() -> TestResult {
    let dir = tempdir()?;
    let root = dir.path();

    let rust_content =
        "pub fn prod() {}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn it_works() {}\n}";
    fs::write(root.join("lib.rs"), rust_content)?;

    r2t_cmd(root)?.assert().success().stdout(contains_all(&[
        "pub fn prod() {}",
        "#[cfg(test)]",
        "mod tests",
        "it_works",
    ]));

    r2t_cmd(root)?
        .arg("--skip-tests")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("pub fn prod() {}")
                .and(contains_none(&["#[cfg(test)]", "mod tests", "it_works"])),
        );

    Ok(())
}

#[test]
fn test_all_formats_comprehensive() -> TestResult {
    test_all_formats(
        |root| {
            fs::create_dir(root.join("src"))?;
            fs::write(
                root.join("src/lib.rs"),
                "pub fn test() {\n    // a comment\n}",
            )?;
            fs::write(root.join("README.md"), "# Project")?;
            Ok(())
        },
        FormatExpectations::new()
            .yaml(&[
                r#"  - full_path: README.md
    content: |
      # Project"#,
                r#"  - full_path: src/lib.rs
    content: |
      pub fn test() {
          // a comment
      }"#,
            ])
            .json(&[
                r#""full_path": "README.md""#,
                r#""content": """
# Project
""""#,
                r#""full_path": "src/lib.rs""#,
                r#""content": """
pub fn test() {
    // a comment
}
""""#,
            ])
            .xml(&[
                "<content full_path=\"README.md\">",
                "# Project\n",
                "</content>",
                "<content full_path=\"src/lib.rs\">",
                "pub fn test() {\n    // a comment\n}\n",
                "</content>",
            ]),
    )
}

#[test]
fn test_format_flags_basic() -> TestResult {
    let dir = tempdir()?;
    let root = dir.path();
    fs::write(root.join("a.txt"), "Hello\nWorld")?;

    r2t_cmd(root)?
        .arg("--format")
        .arg("yaml")
        .assert()
        .success()
        .stdout(contains_all(&[
            "full_path: a.txt",
            r#"content: |
      Hello
      World"#,
        ]));

    r2t_cmd(root)?
        .arg("--format")
        .arg("json")
        .assert()
        .success()
        .stdout(
            contains_all(&[r#""directory":"#, r#""full_path": "a.txt""#]).and(
                predicate::str::is_match(r#"(?s)"content":\s*"""\s*Hello\s*World\s*\n\s*""""#)
                    .unwrap(),
            ),
        );

    r2t_cmd(root)?
        .arg("--format")
        .arg("xml")
        .assert()
        .success()
        .stdout(contains_all(&[
            "<content full_path=\"a.txt\">",
            "Hello\nWorld\n",
            "</content>",
        ]));

    Ok(())
}

#[test]
fn test_format_precedence() -> TestResult {
    let dir = tempdir()?;
    let root = dir.path();
    fs::write(root.join("a.txt"), "Hello")?;

    create_r2t_config(root, "format: json")?;

    r2t_cmd(root)?.assert().success().stdout(
        contains_all(&[r#""directory":"#, r#""full_path": "a.txt""#])
            .and(predicate::str::is_match(r#"(?s)"content":\s*"""\s*Hello\s*\n\s*""""#).unwrap()),
    );

    r2t_cmd(root)?
        .arg("--format")
        .arg("json")
        .assert()
        .success()
        .stdout(
            contains_all(&[r#""directory":"#, r#""full_path": "a.txt""#])
                .and(predicate::str::is_match(r#"(?s)"content":\s*"""\s*Hello\s*\n\s*""""#).unwrap()),
        );

    Ok(())
}

#[test]
fn test_yaml_code_blocks_must_be_literals() -> TestResult {
    let fixture_dir = Path::new("tests/fixtures/normalization");
    r2t_cmd(fixture_dir)?
        .assert()
        .success()
        .stdout(contains_all(&[
            "<content full_path=\"README1.md\">",
            "This is a test",
            "The output should be good and not have newlines everywhere",
            "Blah blah blah",
            "<content full_path=\"README2.md\">",
            "This is another test",
            "The line endings should also be good here",
        ]));

    Ok(())
}