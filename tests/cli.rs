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

/// Checks if any file with the given extension exists in the directory
fn find_file_with_extension(dir: &Path, extension: &str) -> bool {
    fs::read_dir(dir)
        .ok()
        .and_then(|entries| {
            entries
                .filter_map(Result::ok)
                .find(|entry| {
                    entry.path().is_file()
                        && entry
                        .path()
                        .extension()
                        .and_then(|ext| ext.to_str())
                        == Some(extension.trim_start_matches('.'))
                })
        })
        .is_some()
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
    pseudo_xml: Vec<&'static str>,
}

impl FormatExpectations {
    fn new() -> Self {
        Self {
            yaml: Vec::new(),
            json: Vec::new(),
            pseudo_xml: Vec::new(),
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

    fn pseudo_xml(mut self, checks: &[&'static str]) -> Self {
        self.pseudo_xml.extend_from_slice(checks);
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
        ("pseudo-xml", &expectations.pseudo_xml),
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
            "directory:",
            "directory_structure: |",
            "full_path: src/main.rs",
            "content: fn main() {}",
            "full_path: README.md",
            "content: This is a test.",
        ])
            .and(predicate::str::is_match(r"(?m)^\s*[├└]─ .gitignore$").unwrap())
            .and(contains_none(&[
                "full_path: .gitignore",
                "output.log",
            ])),
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

    r2t_cmd(root)?.assert().success().stdout(
        contains_all(&["not_ignored.txt", "I am here"]).and(contains_none(&[
            "full_path: ignored.txt",
            "I should be ignored",
        ])),
    );

    r2t_cmd(root)?
        .arg("--no-gitignore")
        .assert()
        .success()
        .stdout(contains_all(&[
            "not_ignored.txt",
            "I am here",
            "full_path: ignored.txt",
            "content: I should be ignored",
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
        contains_all(&["full_path: src/main.rs", "content: fn main() {}"])
            .and(predicate::str::is_match(r"(?m)^\s*[├└]─ README.md$").unwrap())
            .and(contains_none(&[
                "docs/",
                "guide.md",
                "full_path: README.md",
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
        contains_all(&["full_path: icon.svg", "content: <svg></svg>"])
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
        "full_path: main_test.go",
        "content: package main_test",
        "full_path: src/test/java/AppTest.java",
        "content: public class AppTest {}",
    ]));

    // With --skip-tests: test files appear in tree but not in content
    r2t_cmd(root)?
        .arg("--skip-tests")
        .assert()
        .success()
        .stdout(
            contains_all(&[
                "full_path: main.go",
                "content: package main",
                "full_path: src/main/java/App.java",
                "content: public class App {}",
                "main_test.go",
                "AppTest.java",
            ])
                .and(contains_none(&[
                    "full_path: main_test.go",
                    "package main_test",
                    "full_path: src/test/java/AppTest.java",
                    "public class AppTest {}",
                ])),
        );

    Ok(())
}

#[test]
fn test_skip_tests_rust() -> TestResult {
    let dir = tempdir()?;
    let root = dir.path();

    let rust_content = "pub fn prod() {}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn it_works() {}\n}";
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
                "directory:",
                "directory_structure: |",
                "full_path: src/lib.rs",
                "content: |",
                "    pub fn test() {",
                "        // a comment",
                "    }",
            ])
            .json(&[
                r#""directory":"#,
                r#""content": "pub fn test() {\n    // a comment\n}""#,
            ])
            .pseudo_xml(&[
                "<repo-to-text>",
                "Directory:",
                "<directory_structure>",
                "<content full_path=\"src/lib.rs\">",
                "pub fn test() {",
                "    // a comment",
                "}",
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
            "directory:",
            "full_path: a.txt",
            "content: |",
            "  Hello",
            "  World",
        ]));

    r2t_cmd(root)?
        .arg("--format")
        .arg("json")
        .assert()
        .success()
        .stdout(contains_all(&[
            r#""directory":"#,
            r#""full_path": "a.txt""#,
            r#""content": "Hello\nWorld""#,
        ]));

    r2t_cmd(root)?
        .arg("--format")
        .arg("pseudo-json")
        .assert()
        .success()
        .stdout(
            contains_all(&[r#""directory":"#, r#""full_path": "a.txt""#]).and(
                predicate::str::is_match(r#"(?s)"content":\s*"""\s*Hello\s*World\s*""""#).unwrap(),
            ),
        );

    r2t_cmd(root)?
        .arg("--format")
        .arg("pseudo-xml")
        .assert()
        .success()
        .stdout(contains_all(&[
            "<repo-to-text>",
            "Directory:",
            "<content full_path=\"a.txt\">",
            "Hello",
            "World",
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

    r2t_cmd(root)?
        .assert()
        .success()
        .stdout(contains_all(&[
            r#""directory":"#,
            r#""full_path": "a.txt""#,
            r#""content": "Hello""#,
        ]));

    r2t_cmd(root)?
        .arg("--format")
        .arg("pseudo-json")
        .assert()
        .success()
        .stdout(
            contains_all(&[r#""directory":"#, r#""full_path": "a.txt""#])
                .and(predicate::str::is_match(r#"(?s)"content":\s*"""\s*Hello\s*""""#).unwrap()),
        );

    Ok(())
}

#[test]
fn test_file_output_extensions() -> TestResult {
    let proj_dir = tempdir()?;
    fs::write(proj_dir.path().join("a.txt"), "data")?;

    let test_cases = [
        ("yaml", "yaml"),
        ("json", "json"),
        ("pseudo-json", "txt"),
        ("pseudo-xml", "txt"),
    ];

    for (format, expected_ext) in test_cases {
        let temp_output_dir = tempdir()?;

        Command::cargo_bin("r2t")?
            .arg(proj_dir.path())
            .arg("--output-dir")
            .arg(temp_output_dir.path())
            .arg("--format")
            .arg(format)
            .assert()
            .success();

        assert!(
            find_file_with_extension(temp_output_dir.path(), expected_ext),
            "Expected to find .{} file for format '{}'",
            expected_ext,
            format
        );
    }

    Ok(())
}