use assert_cmd::prelude::*;
use predicates::prelude::*;
use std::fs;
use std::process::Command;
use tempfile::tempdir;

#[test]
fn test_cli_basic_run() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempdir()?;
    let proj_root = dir.path();

    // Setup mock project
    fs::create_dir(proj_root.join("src"))?;
    fs::write(proj_root.join("src/main.rs"), "fn main() {}")?;
    fs::write(proj_root.join("README.md"), "This is a test.")?;
    fs::write(proj_root.join(".gitignore"), "*.log")?;
    fs::write(proj_root.join("output.log"), "some log data")?;

    let mut cmd = Command::cargo_bin("r2t")?;
    cmd.arg(proj_root.to_str().unwrap());
    cmd.arg("--stdout");

    cmd.assert()
        .success()
        .stdout(
            predicate::str::contains("<repo-to-text>")
                .and(predicate::str::contains("<directory_structure>"))
                .and(predicate::str::contains("src/main.rs"))
                .and(predicate::str::contains("fn main() {}"))
                // .gitignore should be in the tree, but its content should NOT be.
                .and(predicate::str::contains(".gitignore"))
                .and(predicate::str::contains("*.log").not())
                // With no custom config, README.md and its content should be present
                .and(predicate::str::contains("README.md"))
                .and(predicate::str::contains("This is a test."))
                // .log is ignored by .gitignore, so it should be absent entirely
                .and(predicate::str::contains("output.log").not()),
        );

    Ok(())
}

#[test]
fn test_cli_no_gitignore_flag() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempdir()?;
    let proj_root = dir.path();

    // Setup mock project
    fs::write(proj_root.join("not_ignored.txt"), "I am here")?;
    fs::write(proj_root.join("ignored.txt"), "I should be ignored")?;
    fs::write(proj_root.join(".gitignore"), "ignored.txt\n")?;

    // 1. Run with default behavior (respect .gitignore)
    let mut cmd = Command::cargo_bin("r2t")?;
    cmd.arg(proj_root.to_str().unwrap());
    cmd.arg("--stdout");

    cmd.assert().success().stdout(
        predicate::str::contains("not_ignored.txt")
            .and(predicate::str::contains("I am here"))
            // Check specifically that `ignored.txt` is not in the tree or content
            .and(predicate::str::contains("├─ ignored.txt").not())
            .and(predicate::str::contains("└─ ignored.txt").not())
            .and(predicate::str::contains("<content full_path=\"ignored.txt\">").not())
            .and(predicate::str::contains("I should be ignored").not()),
    );

    // 2. Run with --no-gitignore flag
    let mut cmd2 = Command::cargo_bin("r2t")?;
    cmd2.arg(proj_root.to_str().unwrap());
    cmd2.arg("--stdout");
    cmd2.arg("--no-gitignore");

    cmd2.assert().success().stdout(
        predicate::str::contains("not_ignored.txt")
            .and(predicate::str::contains("I am here"))
            // Check specifically that `ignored.txt` IS in the tree and content
            .and(predicate::str::is_match(r"(?m)^[├└]─ ignored\.txt$").unwrap())
            .and(predicate::str::contains("I should be ignored")),
    );

    Ok(())
}


#[test]
fn test_r2t_yaml_config_ignores() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempdir()?;
    let proj_root = dir.path();

    // Setup mock project
    fs::create_dir(proj_root.join("src"))?;
    fs::write(proj_root.join("src/main.rs"), "fn main() {}")?;
    fs::write(proj_root.join("README.md"), "This is a test.")?;
    fs::create_dir(proj_root.join("docs"))?;
    fs::write(proj_root.join("docs/guide.md"), "A guide.")?;

    // Create a custom config file
    let config_content = r#"
ignore-tree-and-content:
  - "docs/"

ignore-content:
  - "README.md"
"#;
    fs::write(proj_root.join(".r2t.yaml"), config_content)?;

    let mut cmd = Command::cargo_bin("r2t")?;
    cmd.arg(proj_root.to_str().unwrap());
    cmd.arg("--stdout");

    cmd.assert()
        .success()
        .stdout(
            // `docs/` should be completely ignored from tree and content
            predicate::str::contains("docs/").not()
                .and(predicate::str::contains("guide.md").not())
                // `README.md` should be in the tree, but its content should be ignored
                .and(predicate::str::contains("README.md"))
                .and(predicate::str::contains("This is a test.").not())
                // `src/main.rs` should be fully included
                .and(predicate::str::contains("src/main.rs"))
                .and(predicate::str::contains("fn main() {}")),
        );

    Ok(())
}

#[test]
fn test_binary_file_exclusion() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempdir()?;
    let proj_root = dir.path();

    // A tiny, valid PNG (1x1 pixel)
    let png_data = [
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
        0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
        0x00, 0x1f, 0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0a, 0x49, 0x44, 0x41, 0x54, 0x78,
        0x9c, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00,
        0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];
    fs::write(proj_root.join("logo.png"), &png_data)?;

    // A valid SVG
    fs::write(proj_root.join("icon.svg"), "<svg></svg>")?;

    let mut cmd = Command::cargo_bin("r2t")?;
    cmd.arg(proj_root.to_str().unwrap()).arg("--stdout");

    cmd.assert()
        .success()
        .stdout(
            // SVG should be included
            predicate::str::contains("icon.svg")
                .and(predicate::str::contains("<svg></svg>"))
                // PNG should be completely excluded
                .and(predicate::str::contains("logo.png").not())
        );

    Ok(())
}