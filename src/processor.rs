use crate::config::Config;
use anyhow::{Context, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::{DirEntry as IgnoreDirEntry, WalkBuilder};
use ptree::{write_tree, TreeBuilder};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Result of processing a directory, containing the tree representation
/// and the list of files to include in the output.
#[derive(Debug)]
pub struct ProcessResult {
    pub tree: String,
    pub files_to_include: Vec<PathBuf>,
}

/// A wrapper around directory entries to make them mockable for testing.
#[derive(Clone, Debug)]
pub struct DirectoryEntry {
    path: PathBuf,
    is_dir: bool,
}

impl DirectoryEntry {
    pub fn new(path: PathBuf, is_dir: bool) -> Self {
        Self { path, is_dir }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn is_dir(&self) -> bool {
        self.is_dir
    }

    pub fn is_file(&self) -> bool {
        !self.is_dir
    }

    pub fn file_name(&self) -> Option<&std::ffi::OsStr> {
        self.path.file_name()
    }
}

impl From<IgnoreDirEntry> for DirectoryEntry {
    fn from(entry: IgnoreDirEntry) -> Self {
        let path = entry.path().to_path_buf();
        let is_dir = entry.file_type().map_or(false, |ft| ft.is_dir());
        Self::new(path, is_dir)
    }
}

/// Trait abstraction for directory walking to enable testing.
pub trait DirectoryWalker {
    fn walk(&self, path: &Path, no_gitignore: bool) -> Result<Vec<DirectoryEntry>>;
}

/// Trait abstraction for binary file checking to enable testing.
pub trait BinaryChecker {
    fn is_binary_or_image(&self, path: &Path) -> Result<bool>;
}

/// Production implementation of DirectoryWalker using the `ignore` crate.
#[derive(Default)]
pub struct IgnoreWalker;

impl DirectoryWalker for IgnoreWalker {
    fn walk(&self, path: &Path, no_gitignore: bool) -> Result<Vec<DirectoryEntry>> {
        WalkBuilder::new(path)
            .hidden(false)
            .git_ignore(!no_gitignore)
            .require_git(false)
            .git_global(false)
            .git_exclude(false)
            .sort_by_file_path(std::cmp::Ord::cmp)
            .build()
            .map(|result| {
                result
                    .map(DirectoryEntry::from)
                    .context("Failed to process directory entry")
            })
            .collect()
    }
}

/// Production implementation of BinaryChecker.
#[derive(Default)]
pub struct FileBinaryChecker;

impl BinaryChecker for FileBinaryChecker {
    fn is_binary_or_image(&self, path: &Path) -> Result<bool> {
        crate::files::is_binary_or_image(path)
    }
}

/// Main processor struct with dependency injection for testability.
pub struct DirectoryProcessor<W = IgnoreWalker, B = FileBinaryChecker>
where
    W: DirectoryWalker,
    B: BinaryChecker,
{
    walker: W,
    binary_checker: B,
}

impl Default for DirectoryProcessor {
    fn default() -> Self {
        Self {
            walker: IgnoreWalker,
            binary_checker: FileBinaryChecker,
        }
    }
}

impl DirectoryProcessor {
    /// Creates a new processor with production dependencies.
    pub fn new() -> Self {
        Self::default()
    }
}

impl<W, B> DirectoryProcessor<W, B>
where
    W: DirectoryWalker,
    B: BinaryChecker,
{
    /// Creates a processor with custom dependencies (for testing).
    #[cfg(test)]
    pub fn with_deps(walker: W, binary_checker: B) -> Self {
        Self {
            walker,
            binary_checker,
        }
    }

    /// Processes a directory and returns the tree representation and files to include.
    pub fn process(
        &self,
        path: &Path,
        config: &Config,
        no_gitignore: bool,
        skip_tests: bool,
    ) -> Result<ProcessResult> {
        // Build ignore patterns
        let content_only_patterns = Self::build_content_only_patterns(config, skip_tests);
        let tree_and_content_patterns = Self::build_tree_and_content_patterns(config);

        let tree_and_content_ignores = build_glob_set(&tree_and_content_patterns)
            .context("Failed to build tree and content ignore patterns")?;
        let content_only_ignores = build_glob_set(&content_only_patterns)
            .context("Failed to build content-only ignore patterns")?;

        // Walk the directory
        let entries = self
            .walker
            .walk(path, no_gitignore)
            .context("Failed to walk directory")?;

        // Process entries
        let (tree_nodes, files_to_include) = self.process_entries(
            entries,
            path,
            &tree_and_content_ignores,
            &content_only_ignores,
        )?;

        // Build tree representation
        let tree = Self::build_tree_string(path, &tree_nodes)?;

        Ok(ProcessResult {
            tree,
            files_to_include,
        })
    }

    /// Builds the list of patterns that should be excluded from content only.
    fn build_content_only_patterns(config: &Config, skip_tests: bool) -> Vec<String> {
        const CONFIG_FILES: &[&str] = &[".r2t.yaml", ".gitignore"];

        const TEST_PATTERNS: &[&str] = &[
            // Go
            "*_test.go",
            // Java
            "src/test/**",
            "src/testIntegration/**",
            "src/testApplication/**",
            // Python
            "test_*.py",
            "**/test/**/*.py",
            "tests/**/*.py",
        ];

        let mut patterns = config.ignore_content.clone();
        patterns.extend(CONFIG_FILES.iter().copied().map(String::from));

        if skip_tests {
            patterns.extend(TEST_PATTERNS.iter().copied().map(String::from));
        }

        patterns
    }

    /// Builds the list of patterns that should be excluded from both tree and content.
    fn build_tree_and_content_patterns(config: &Config) -> Vec<String> {
        let mut patterns = config.ignore_tree_and_content.clone();
        patterns.push(".git/".to_string());
        patterns
    }

    /// Processes directory entries and categorizes them for tree and content inclusion.
    fn process_entries(
        &self,
        entries: Vec<DirectoryEntry>,
        base_path: &Path,
        tree_and_content_ignores: &GlobSet,
        content_only_ignores: &GlobSet,
    ) -> Result<(HashMap<PathBuf, Vec<DirectoryEntry>>, Vec<PathBuf>)> {
        let mut tree_nodes: HashMap<PathBuf, Vec<DirectoryEntry>> = HashMap::new();
        let mut files_to_include = Vec::new();

        for entry in entries {
            // Skip the root directory itself (we only want its contents)
            if entry.path() == base_path {
                continue;
            }

            // Check if should be included in tree
            if !self.should_include_in_tree(&entry, base_path, tree_and_content_ignores)? {
                continue;
            }

            // Add to tree structure
            if let Some(parent) = entry.path().parent() {
                tree_nodes
                    .entry(parent.to_path_buf())
                    .or_default()
                    .push(entry.clone());
            }

            // Check if should be included in content (only files, not directories)
            if entry.is_file()
                && Self::should_include_in_content(&entry, base_path, content_only_ignores)?
            {
                files_to_include.push(entry.path().to_path_buf());
            }
        }

        Ok((tree_nodes, files_to_include))
    }

    /// Gets the relative path of an entry with proper error handling.
    fn get_relative_path<'a>(entry: &'a DirectoryEntry, base_path: &Path) -> Result<&'a Path> {
        entry.path().strip_prefix(base_path).with_context(|| {
            format!(
                "Path '{}' is not under base path '{}'",
                entry.path().display(),
                base_path.display()
            )
        })
    }

    /// Determines if an entry should be included in the tree representation.
    fn should_include_in_tree(
        &self,
        entry: &DirectoryEntry,
        base_path: &Path,
        tree_and_content_ignores: &GlobSet,
    ) -> Result<bool> {
        let relative_path = Self::get_relative_path(entry, base_path)?;

        // Skip if matches tree+content ignore patterns
        if matches_ignore_pattern(relative_path, tree_and_content_ignores, entry.is_dir()) {
            return Ok(false);
        }

        // Skip binary files (but only check actual files, not directories)
        if entry.is_file() && self.binary_checker.is_binary_or_image(entry.path())? {
            return Ok(false);
        }

        Ok(true)
    }

    /// Determines if an entry should be included in the content output.
    fn should_include_in_content(
        entry: &DirectoryEntry,
        base_path: &Path,
        content_only_ignores: &GlobSet,
    ) -> Result<bool> {
        let relative_path = Self::get_relative_path(entry, base_path)?;
        Ok(!matches_ignore_pattern(
            relative_path,
            content_only_ignores,
            false,
        ))
    }

    /// Builds a string representation of the directory tree.
    fn build_tree_string(
        path: &Path,
        tree_nodes: &HashMap<PathBuf, Vec<DirectoryEntry>>,
    ) -> Result<String> {
        let root_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();

        let mut tree_builder = TreeBuilder::new(root_name);
        build_ptree_recursive(path, tree_nodes, &mut tree_builder);

        let tree_item = tree_builder.build();
        let mut buffer = Vec::new();
        write_tree(&tree_item, &mut buffer).context("Failed to write tree")?;

        String::from_utf8(buffer).context("Tree output is not valid UTF-8")
    }
}

/// Builds a GlobSet from a list of patterns.
///
/// For patterns ending with '/', it creates additional patterns to match:
/// - The directory itself (without trailing slash)
/// - All contents recursively (with /**)
fn build_glob_set(patterns: &[String]) -> Result<GlobSet, globset::Error> {
    let mut builder = GlobSetBuilder::new();

    for pattern in patterns {
        builder.add(Glob::new(pattern)?);

        // For directory patterns, add recursive matching
        if let Some(dir_pattern) = pattern.strip_suffix('/') {
            builder.add(Glob::new(dir_pattern)?);
            builder.add(Glob::new(&format!("{}/**", dir_pattern))?);
        }
    }

    builder.build()
}

/// Checks if a path matches any pattern in the glob set.
///
/// For directories, also checks with a trailing slash.
fn matches_ignore_pattern(relative_path: &Path, glob_set: &GlobSet, is_dir: bool) -> bool {
    glob_set.is_match(relative_path)
        || (is_dir && glob_set.is_match(format!("{}/", relative_path.display()).as_str()))
}

/// Recursively builds a ptree representation of the directory structure.
fn build_ptree_recursive(
    current_path: &Path,
    nodes: &HashMap<PathBuf, Vec<DirectoryEntry>>,
    builder: &mut TreeBuilder,
) {
    if let Some(children) = nodes.get(current_path) {
        for child in children {
            let child_name = child
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("<unnamed>")
                .to_string();

            if child.is_dir() {
                builder.begin_child(child_name);
                build_ptree_recursive(child.path(), nodes, builder);
                builder.end_child();
            } else {
                builder.add_empty_child(child_name);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockall::mock;
    use crate::cli::FormatArg::{Xml};

    // Helper function to create a test config
    fn create_test_config(
        ignore_tree_and_content: Vec<&str>,
        ignore_content: Vec<&str>,
    ) -> Config {
        Config {
            format: Some(Xml),
            ignore_tree_and_content: ignore_tree_and_content
                .into_iter()
                .map(String::from)
                .collect(),
            ignore_content: ignore_content.into_iter().map(String::from).collect(),
        }
    }

    // Mock implementations
    mock! {
        pub DirectoryWalker {}
        impl DirectoryWalker for DirectoryWalker {
            fn walk(&self, path: &Path, no_gitignore: bool) -> Result<Vec<DirectoryEntry>>;
        }
    }

    mock! {
        pub BinaryChecker {}
        impl BinaryChecker for BinaryChecker {
            fn is_binary_or_image(&self, path: &Path) -> Result<bool>;
        }
    }

    #[test]
    fn test_build_glob_set_simple_pattern() {
        let patterns = vec!["*.txt".to_string(), "*.log".to_string()];
        let glob_set = build_glob_set(&patterns).unwrap();

        assert!(glob_set.is_match("file.txt"));
        assert!(glob_set.is_match("file.log"));
        assert!(!glob_set.is_match("file.rs"));
    }

    #[test]
    fn test_build_glob_set_directory_pattern() {
        let patterns = vec!["node_modules/".to_string()];
        let glob_set = build_glob_set(&patterns).unwrap();

        assert!(glob_set.is_match("node_modules"));
        assert!(glob_set.is_match("node_modules/package.json"));
        assert!(glob_set.is_match("node_modules/subdir/file.js"));
    }

    #[test]
    fn test_build_glob_set_nested_directory_pattern() {
        let patterns = vec!["src/generated/".to_string()];
        let glob_set = build_glob_set(&patterns).unwrap();

        assert!(glob_set.is_match("src/generated"));
        assert!(glob_set.is_match("src/generated/file.rs"));
        assert!(glob_set.is_match("src/generated/deep/nested/file.rs"));
    }

    #[test]
    fn test_build_glob_set_multiple_patterns() {
        let patterns = vec![
            "*.log".to_string(),
            "target/".to_string(),
            "*.tmp".to_string(),
        ];
        let glob_set = build_glob_set(&patterns).unwrap();

        assert!(glob_set.is_match("debug.log"));
        assert!(glob_set.is_match("target"));
        assert!(glob_set.is_match("target/debug/app"));
        assert!(glob_set.is_match("temp.tmp"));
        assert!(!glob_set.is_match("src/main.rs"));
    }

    #[test]
    fn test_matches_ignore_pattern_file() {
        let patterns = vec!["*.log".to_string(), "temp.txt".to_string()];
        let glob_set = build_glob_set(&patterns).unwrap();

        assert!(matches_ignore_pattern(Path::new("debug.log"), &glob_set, false));
        assert!(matches_ignore_pattern(Path::new("temp.txt"), &glob_set, false));
        assert!(!matches_ignore_pattern(Path::new("main.rs"), &glob_set, false));
    }

    #[test]
    fn test_matches_ignore_pattern_directory() {
        let patterns = vec!["node_modules/".to_string()];
        let glob_set = build_glob_set(&patterns).unwrap();

        assert!(matches_ignore_pattern(Path::new("node_modules"), &glob_set, true));
        assert!(matches_ignore_pattern(Path::new("node_modules/package"), &glob_set, false));
        assert!(!matches_ignore_pattern(Path::new("src"), &glob_set, true));
    }

    #[test]
    fn test_build_glob_set_with_dot_files() {
        let patterns = vec![".env".to_string(), ".git/".to_string()];
        let glob_set = build_glob_set(&patterns).unwrap();

        assert!(glob_set.is_match(".env"));
        assert!(glob_set.is_match(".git"));
        assert!(glob_set.is_match(".git/config"));
    }

    #[test]
    fn test_matches_ignore_pattern_with_forward_slash() {
        let patterns = vec!["src/generated/".to_string()];
        let glob_set = build_glob_set(&patterns).unwrap();

        let path = PathBuf::from("src").join("generated");
        assert!(matches_ignore_pattern(&path, &glob_set, true));

        let path = PathBuf::from("src").join("generated").join("file.rs");
        assert!(matches_ignore_pattern(&path, &glob_set, false));
    }

    #[test]
    fn test_process_directory_with_tree_and_content_ignore() -> Result<()> {
        let base_path = PathBuf::from("/test");

        let entries = vec![
            DirectoryEntry::new(base_path.join("include.txt"), false),
            DirectoryEntry::new(base_path.join("exclude.log"), false),
            DirectoryEntry::new(base_path.join("node_modules"), true),
            DirectoryEntry::new(base_path.join("node_modules/package.json"), false),
        ];

        let mut mock_walker = MockDirectoryWalker::new();
        mock_walker
            .expect_walk()
            .times(1)
            .return_once(move |_, _| Ok(entries));

        let mut mock_binary = MockBinaryChecker::new();
        mock_binary
            .expect_is_binary_or_image()
            .returning(|_| Ok(false));

        let processor =
            DirectoryProcessor::with_deps(mock_walker, mock_binary);

        let config = create_test_config(vec!["*.log", "node_modules/"], vec![]);
        let result = processor.process(&base_path, &config, false, false)?;

        // Check that .log file is excluded from both tree and content
        assert!(!result.files_to_include.iter().any(|p| p.ends_with("exclude.log")));
        assert!(!result.tree.contains("exclude.log"));

        // Check that node_modules is excluded from tree
        assert!(!result.tree.contains("node_modules"));

        // Check that include.txt is present
        assert!(result.files_to_include.iter().any(|p| p.ends_with("include.txt")));

        Ok(())
    }

    #[test]
    fn test_process_directory_with_content_only_ignore() -> Result<()> {
        let base_path = PathBuf::from("/test");

        let entries = vec![
            DirectoryEntry::new(base_path.join("visible.txt"), false),
            DirectoryEntry::new(base_path.join("hidden.txt"), false),
        ];

        let mut mock_walker = MockDirectoryWalker::new();
        mock_walker
            .expect_walk()
            .times(1)
            .return_once(move |_, _| Ok(entries));

        let mut mock_binary = MockBinaryChecker::new();
        mock_binary
            .expect_is_binary_or_image()
            .returning(|_| Ok(false));

        let processor =
            DirectoryProcessor::with_deps(mock_walker, mock_binary);

        let config = create_test_config(vec![], vec!["hidden.txt"]);
        let result = processor.process(&base_path, &config, false, false)?;

        // hidden.txt should be in tree but not in files_to_include
        assert!(result.tree.contains("hidden.txt"));
        assert!(!result.files_to_include.iter().any(|p| p.ends_with("hidden.txt")));

        // visible.txt should be in both
        assert!(result.tree.contains("visible.txt"));
        assert!(result.files_to_include.iter().any(|p| p.ends_with("visible.txt")));

        Ok(())
    }

    #[test]
    fn test_process_directory_excludes_r2t_yaml_from_content() -> Result<()> {
        let base_path = PathBuf::from("/test");

        let entries = vec![
            DirectoryEntry::new(base_path.join(".r2t.yaml"), false),
            DirectoryEntry::new(base_path.join("other.txt"), false),
        ];

        let mut mock_walker = MockDirectoryWalker::new();
        mock_walker
            .expect_walk()
            .times(1)
            .return_once(move |_, _| Ok(entries));

        let mut mock_binary = MockBinaryChecker::new();
        mock_binary
            .expect_is_binary_or_image()
            .returning(|_| Ok(false));

        let processor =
            DirectoryProcessor::with_deps(mock_walker, mock_binary);

        let config = create_test_config(vec![], vec![]);
        let result = processor.process(&base_path, &config, false, false)?;

        // .r2t.yaml should be in tree but not in content
        assert!(result.tree.contains(".r2t.yaml"));
        assert!(!result.files_to_include.iter().any(|p| p.ends_with(".r2t.yaml")));

        // other.txt should be in both
        assert!(result.files_to_include.iter().any(|p| p.ends_with("other.txt")));

        Ok(())
    }

    #[test]
    fn test_process_directory_excludes_binary_files() -> Result<()> {
        let base_path = PathBuf::from("/test");

        let entries = vec![
            DirectoryEntry::new(base_path.join("text.txt"), false),
            DirectoryEntry::new(base_path.join("image.png"), false),
        ];

        let mut mock_walker = MockDirectoryWalker::new();
        mock_walker
            .expect_walk()
            .times(1)
            .return_once(move |_, _| Ok(entries));

        // Mock binary checker to return true for .png files
        let mut mock_binary = MockBinaryChecker::new();
        mock_binary
            .expect_is_binary_or_image()
            .returning(|path| Ok(path.extension().and_then(|s| s.to_str()) == Some("png")));

        let processor =
            DirectoryProcessor::with_deps(mock_walker, mock_binary);

        let config = create_test_config(vec![], vec![]);
        let result = processor.process(&base_path, &config, false, false)?;

        // Binary files should be excluded from tree and content
        assert!(!result.tree.contains("image.png"));
        assert!(!result.files_to_include.iter().any(|p| p.ends_with("image.png")));

        // Text files should be included
        assert!(result.tree.contains("text.txt"));
        assert!(result.files_to_include.iter().any(|p| p.ends_with("text.txt")));

        Ok(())
    }

    #[test]
    fn test_process_directory_nested_structure() -> Result<()> {
        let base_path = PathBuf::from("/test");

        let entries = vec![
            DirectoryEntry::new(base_path.join("src"), true),
            DirectoryEntry::new(base_path.join("src/models"), true),
            DirectoryEntry::new(base_path.join("src/controllers"), true),
            DirectoryEntry::new(base_path.join("src/main.rs"), false),
            DirectoryEntry::new(base_path.join("src/models/user.rs"), false),
            DirectoryEntry::new(base_path.join("src/controllers/api.rs"), false),
        ];

        let mut mock_walker = MockDirectoryWalker::new();
        mock_walker
            .expect_walk()
            .times(1)
            .return_once(move |_, _| Ok(entries));

        let mut mock_binary = MockBinaryChecker::new();
        mock_binary
            .expect_is_binary_or_image()
            .returning(|_| Ok(false));

        let processor =
            DirectoryProcessor::with_deps(mock_walker, mock_binary);

        let config = create_test_config(vec![], vec![]);
        let result = processor.process(&base_path, &config, false, false)?;

        // Check nested structure in tree
        assert!(result.tree.contains("src"));
        assert!(result.tree.contains("models"));
        assert!(result.tree.contains("controllers"));
        assert!(result.tree.contains("main.rs"));
        assert!(result.tree.contains("user.rs"));
        assert!(result.tree.contains("api.rs"));

        // Check all files are included
        assert_eq!(
            result
                .files_to_include
                .iter()
                .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("rs"))
                .count(),
            3
        );

        Ok(())
    }

    #[test]
    fn test_process_directory_empty_directory() -> Result<()> {
        let base_path = PathBuf::from("/test");

        let entries = vec![];

        let mut mock_walker = MockDirectoryWalker::new();
        mock_walker
            .expect_walk()
            .times(1)
            .return_once(move |_, _| Ok(entries));

        let mut mock_binary = MockBinaryChecker::new();
        mock_binary.expect_is_binary_or_image().times(0);

        let processor =
            DirectoryProcessor::with_deps(mock_walker, mock_binary);

        let config = create_test_config(vec![], vec![]);
        let result = processor.process(&base_path, &config, false, false)?;

        assert!(result.files_to_include.is_empty());
        assert!(!result.tree.is_empty()); // Tree still has root

        Ok(())
    }

    #[test]
    fn test_process_directory_with_multiple_ignore_types() -> Result<()> {
        let base_path = PathBuf::from("/test");

        let entries = vec![
            DirectoryEntry::new(base_path.join("show_in_tree_and_content.txt"), false),
            DirectoryEntry::new(base_path.join("show_in_tree_only.txt"), false),
            DirectoryEntry::new(base_path.join("hide_completely.txt"), false),
        ];

        let mut mock_walker = MockDirectoryWalker::new();
        mock_walker
            .expect_walk()
            .times(1)
            .return_once(move |_, _| Ok(entries));

        let mut mock_binary = MockBinaryChecker::new();
        mock_binary
            .expect_is_binary_or_image()
            .returning(|_| Ok(false));

        let processor =
            DirectoryProcessor::with_deps(mock_walker, mock_binary);

        let config = create_test_config(vec!["hide_completely.txt"], vec!["show_in_tree_only.txt"]);
        let result = processor.process(&base_path, &config, false, false)?;

        assert!(result.tree.contains("show_in_tree_and_content.txt"));
        assert!(result
            .files_to_include
            .iter()
            .any(|p| p.ends_with("show_in_tree_and_content.txt")));

        assert!(result.tree.contains("show_in_tree_only.txt"));
        assert!(!result
            .files_to_include
            .iter()
            .any(|p| p.ends_with("show_in_tree_only.txt")));

        assert!(!result.tree.contains("hide_completely.txt"));
        assert!(!result
            .files_to_include
            .iter()
            .any(|p| p.ends_with("hide_completely.txt")));

        Ok(())
    }

    #[test]
    fn test_walker_error_propagation() {
        let base_path = PathBuf::from("/test");

        let mut mock_walker = MockDirectoryWalker::new();
        mock_walker
            .expect_walk()
            .times(1)
            .return_once(|_, _| Err(anyhow::anyhow!("Walker error")));

        let mock_binary = MockBinaryChecker::new();

        let processor =
            DirectoryProcessor::with_deps(mock_walker, mock_binary);

        let config = create_test_config(vec![], vec![]);
        let result = processor.process(&base_path, &config, false, false);

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Failed to walk directory"));
    }

    #[test]
    fn test_binary_checker_error_propagation() {
        let base_path = PathBuf::from("/test");

        let entries = vec![DirectoryEntry::new(base_path.join("test.txt"), false)];

        let mut mock_walker = MockDirectoryWalker::new();
        mock_walker
            .expect_walk()
            .times(1)
            .return_once(move |_, _| Ok(entries));

        let mut mock_binary = MockBinaryChecker::new();
        mock_binary
            .expect_is_binary_or_image()
            .times(1)
            .return_once(|_| Err(anyhow::anyhow!("Binary check error")));

        let processor =
            DirectoryProcessor::with_deps(mock_walker, mock_binary);

        let config = create_test_config(vec![], vec![]);
        let result = processor.process(&base_path, &config, false, false);

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Binary check error"));
    }

    #[test]
    fn test_skip_tests_flag() -> Result<()> {
        let base_path = PathBuf::from("/test");

        let entries = vec![
            DirectoryEntry::new(base_path.join("main.go"), false),
            DirectoryEntry::new(base_path.join("main_test.go"), false),
            DirectoryEntry::new(base_path.join("src/test/helper.go"), false),
            DirectoryEntry::new(base_path.join("test_example.py"), false),
            DirectoryEntry::new(base_path.join("app.py"), false),
        ];

        let mut mock_walker = MockDirectoryWalker::new();
        mock_walker
            .expect_walk()
            .times(1)
            .return_once(move |_, _| Ok(entries));

        let mut mock_binary = MockBinaryChecker::new();
        mock_binary
            .expect_is_binary_or_image()
            .returning(|_| Ok(false));

        let processor =
            DirectoryProcessor::with_deps(mock_walker, mock_binary);

        let config = create_test_config(vec![], vec![]);
        let result = processor.process(&base_path, &config, false, true)?;

        // Test files should be in tree but not in content
        assert!(result.tree.contains("main_test.go"));
        assert!(!result.files_to_include.iter().any(|p| p.ends_with("main_test.go")));

        assert!(result.tree.contains("test_example.py"));
        assert!(!result.files_to_include.iter().any(|p| p.ends_with("test_example.py")));

        // Regular files should be included
        assert!(result.files_to_include.iter().any(|p| p.ends_with("main.go")));
        assert!(result.files_to_include.iter().any(|p| p.ends_with("app.py")));

        Ok(())
    }

    #[test]
    fn test_binary_check_not_called_for_directories() -> Result<()> {
        let base_path = PathBuf::from("/test");

        let entries = vec![
            DirectoryEntry::new(base_path.join("src"), true),
            DirectoryEntry::new(base_path.join("src/main.rs"), false),
        ];

        let mut mock_walker = MockDirectoryWalker::new();
        mock_walker
            .expect_walk()
            .times(1)
            .return_once(move |_, _| Ok(entries));

        let mut mock_binary = MockBinaryChecker::new();
        // Expect is_binary_or_image to be called only once for the file, not the directory
        mock_binary
            .expect_is_binary_or_image()
            .times(1)
            .returning(|_| Ok(false));

        let processor =
            DirectoryProcessor::with_deps(mock_walker, mock_binary);

        let config = create_test_config(vec![], vec![]);
        let result = processor.process(&base_path, &config, false, false)?;

        assert!(result.tree.contains("src"));
        assert!(result.tree.contains("main.rs"));

        Ok(())
    }

    #[test]
    fn test_unicode_filenames() -> Result<()> {
        let base_path = PathBuf::from("/test");

        let entries = vec![
            DirectoryEntry::new(base_path.join("测试.txt"), false),
            DirectoryEntry::new(base_path.join("файл.rs"), false),
            DirectoryEntry::new(base_path.join("🚀_rocket.md"), false),
        ];

        let mut mock_walker = MockDirectoryWalker::new();
        mock_walker
            .expect_walk()
            .times(1)
            .return_once(move |_, _| Ok(entries));

        let mut mock_binary = MockBinaryChecker::new();
        mock_binary
            .expect_is_binary_or_image()
            .returning(|_| Ok(false));

        let processor =
            DirectoryProcessor::with_deps(mock_walker, mock_binary);

        let config = create_test_config(vec![], vec![]);
        let result = processor.process(&base_path, &config, false, false)?;

        // Unicode filenames should be handled correctly
        assert!(result.tree.contains("测试.txt"));
        assert!(result.tree.contains("файл.rs"));
        assert!(result.tree.contains("🚀_rocket.md"));

        assert_eq!(result.files_to_include.len(), 3);

        Ok(())
    }

    #[test]
    fn test_path_not_under_base_path_error() {
        let base_path = PathBuf::from("/test");

        // Entry with path outside the base path
        let entries = vec![
            DirectoryEntry::new(PathBuf::from("/other/file.txt"), false),
        ];

        let mut mock_walker = MockDirectoryWalker::new();
        mock_walker
            .expect_walk()
            .times(1)
            .return_once(move |_, _| Ok(entries));

        let mut mock_binary = MockBinaryChecker::new();
        mock_binary
            .expect_is_binary_or_image()
            .returning(|_| Ok(false));

        let processor =
            DirectoryProcessor::with_deps(mock_walker, mock_binary);

        let config = create_test_config(vec![], vec![]);
        let result = processor.process(&base_path, &config, false, false);

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("not under base path"));
    }
}