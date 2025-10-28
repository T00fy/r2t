use std::path::Path;

/// Strips `#[cfg(test)]` modules from Rust file content.
/// Returns the content unchanged for non-Rust files.
///
/// # Limitations
/// This implementation uses simple text parsing and may not handle all edge cases
/// correctly (e.g., braces in strings/comments, complex macros).
/// TODO: we should consider using a proper Rust parser like `syn`.
pub fn strip_inline_tests(path: &Path, content: &str) -> String {
    // Early return for non-Rust files
    if !path.extension().is_some_and(|ext| ext == "rs") {
        return content.to_string();
    }

    let mut output = String::with_capacity(content.len()); // Pre-allocate
    let mut lines = content.lines().peekable();
    let mut skip_state = SkipState::NotSkipping;

    while let Some(line) = lines.next() {
        match skip_state {
            SkipState::Skipping { mut depth } => {
                // Count braces while skipping
                depth += line.matches('{').count() as i32;
                depth -= line.matches('}').count() as i32;

                if depth <= 0 {
                    // Exited the test block
                    skip_state = SkipState::JustExited;
                } else {
                    skip_state = SkipState::Skipping { depth };
                }
            }
            SkipState::JustExited => {
                // Skip one trailing blank line, then reset
                skip_state = SkipState::NotSkipping;
                if !line.trim().is_empty() {
                    output.push_str(line);
                    output.push('\n');
                }
            }
            SkipState::NotSkipping => {
                if is_test_module_start(line, lines.peek()) {
                    skip_state = enter_skip_state(line, lines.peek());
                    if matches!(skip_state, SkipState::Skipping { .. }) {
                        // Consume the next line if it's the mod declaration
                        if line.trim() == "#[cfg(test)]" {
                            lines.next();
                        }
                    }
                } else {
                    output.push_str(line);
                    output.push('\n');
                }
            }
        }
    }

    output
}

#[derive(Debug, Clone, Copy)]
enum SkipState {
    NotSkipping,
    Skipping { depth: i32 },
    JustExited,
}

/// Checks if the current line (and optionally the next) starts a test module
fn is_test_module_start(line: &str, next_line: Option<&&str>) -> bool {
    let trimmed = line.trim();

    // Case 1: "#[cfg(test)]" followed by "mod tests {" on next line
    if trimmed == "#[cfg(test)]" {
        if let Some(next) = next_line {
            return next.trim().starts_with("mod ") && next.contains('{');
        }
    }

    // Case 2: One-liner "#[cfg(test)] mod tests { ... }"
    trimmed.starts_with("#[cfg(test)]") && trimmed.contains("mod ") && trimmed.contains('{')
}

/// Determines the skip state when entering a test block
fn enter_skip_state(line: &str, next_line: Option<&&str>) -> SkipState {
    let trimmed = line.trim();

    // Find the line containing the opening brace
    let brace_line = if trimmed == "#[cfg(test)]" {
        next_line.map(|s| *s).unwrap_or("")
    } else {
        line
    };

    // Count braces on the line
    let open_braces = brace_line.matches('{').count() as i32;
    let close_braces = brace_line.matches('}').count() as i32;
    let depth = open_braces - close_braces;

    if depth > 0 {
        SkipState::Skipping { depth }
    } else {
        // One-line test block
        SkipState::JustExited
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_strip_rust_simple_case() {
        let content = "fn add(a: i32, b: i32) -> i32 {
    a + b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add() {
        assert_eq!(add(2, 2), 4);
    }
}
";
        let expected = "fn add(a: i32, b: i32) -> i32 {
    a + b
}

";
        let path = PathBuf::from("lib.rs");
        let result = strip_inline_tests(&path, content);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_strip_rust_with_nested_braces() {
        let content = "fn my_func() {}

#[cfg(test)]
mod tests {
    #[test]
    fn a_test() {
        if true {
            assert!(true);
        }
    }
}

fn another_func() {}
";
        let expected = "fn my_func() {}

fn another_func() {}
";
        let path = PathBuf::from("lib.rs");
        let result = strip_inline_tests(&path, content);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_no_stripping_for_non_rust_files() {
        let content = "#[cfg(test)] mod tests {}";
        let path = PathBuf::from("lib.txt");
        let result = strip_inline_tests(&path, content);
        assert_eq!(result, content);
    }

    #[test]
    fn test_no_test_block() {
        let content = "fn main() { println!(\"Hello\"); }";
        let path = PathBuf::from("main.rs");
        let result = strip_inline_tests(&path, content);
        assert_eq!(result, format!("{}\n", content));
    }

    #[test]
    fn test_one_line_test_mod() {
        let content = "fn main() {}

#[cfg(test)] mod tests { #[test] fn it_works() {} }

fn after() {}
";
        let expected = "fn main() {}

fn after() {}
";
        let path = PathBuf::from("main.rs");
        let result = strip_inline_tests(&path, content);
        assert_eq!(result, expected);
    }
}