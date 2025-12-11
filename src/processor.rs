use crate::config::Config;
use crate::resolver::{ConfigResolver, PathConfig};
use anyhow::{Context, Result};
use ignore::{DirEntry as IgnoreDirEntry, WalkBuilder};
use ptree::{write_tree, TreeBuilder};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

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
        no_merge: bool,
    ) -> Result<ProcessResult> {
        let mut root_config = config.clone();
        let content_only_extras = Self::get_content_only_extras(skip_tests);
        root_config.ignore_content.extend(content_only_extras);

        let tree_extras = Self::get_tree_and_content_extras();
        root_config.ignore_tree_and_content.extend(tree_extras);

        let mut resolver = ConfigResolver::new(root_config, path.to_path_buf(), no_merge)
            .context("Failed to initialize config resolver")?;

        let entries = self
            .walker
            .walk(path, no_gitignore)
            .context("Failed to walk directory")?;

        let (tree_nodes, files_to_include) = self.process_entries(
            entries,
            path,
            &mut resolver,
        )?;

        let tree = Self::build_tree_string(path, &tree_nodes)?;

        Ok(ProcessResult {
            tree,
            files_to_include,
        })
    }

    fn get_content_only_extras(skip_tests: bool) -> Vec<String> {
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

        let mut patterns: Vec<String> = CONFIG_FILES.iter().copied().map(String::from).collect();

        if skip_tests {
            patterns.extend(TEST_PATTERNS.iter().copied().map(String::from));
        }

        patterns
    }

    fn get_tree_and_content_extras() -> Vec<String> {
        vec![".git/".to_string()]
    }

    /// Processes directory entries and categorizes them for tree and content inclusion.
    fn process_entries(
        &self,
        entries: Vec<DirectoryEntry>,
        base_path: &Path,
        resolver: &mut ConfigResolver,
    ) -> Result<(HashMap<PathBuf, Vec<DirectoryEntry>>, Vec<PathBuf>)> {
        let mut tree_nodes: HashMap<PathBuf, Vec<DirectoryEntry>> = HashMap::new();
        let mut files_to_include = Vec::new();

        for entry in entries {
            if entry.path() == base_path {
                continue;
            }
            let config = resolver.get_config(entry.path())?;

            if !self.should_include_in_tree(&entry, base_path, &config)? {
                continue;
            }

            if let Some(parent) = entry.path().parent() {
                tree_nodes
                    .entry(parent.to_path_buf())
                    .or_default()
                    .push(entry.clone());
            }

            if entry.is_file()
                && Self::should_include_in_content(&entry, base_path, &config)?
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
        config: &Arc<PathConfig>,
    ) -> Result<bool> {
        let relative_path = Self::get_relative_path(entry, base_path)?;
        if matches_ignore_pattern(relative_path, &config.tree_and_content_ignores, entry.is_dir()) {
            return Ok(false);
        }
        if entry.is_file() && self.binary_checker.is_binary_or_image(entry.path())? {
            return Ok(false);
        }

        Ok(true)
    }

    /// Determines if an entry should be included in the content output.
    fn should_include_in_content(
        entry: &DirectoryEntry,
        base_path: &Path,
        config: &Arc<PathConfig>,
    ) -> Result<bool> {
        let relative_path = Self::get_relative_path(entry, base_path)?;
        Ok(!matches_ignore_pattern(
            relative_path,
            &config.content_only_ignores,
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

/// Checks if a path matches any pattern in the glob set.
///
/// For directories, also checks with a trailing slash.
fn matches_ignore_pattern(relative_path: &Path, glob_set: &globset::GlobSet, is_dir: bool) -> bool {
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
    use tempfile::TempDir;

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

    fn create_entry(root: &Path, rel_path: &str, is_dir: bool) -> DirectoryEntry {
        DirectoryEntry::new(root.join(rel_path), is_dir)
    }

    #[test]
    fn test_content_exclusion_appears_in_tree_only() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        let config = Config {
            ignore_content: vec!["*.lock".to_string()],
            ..Default::default()
        };

        // Mocks
        let mut walker = MockDirectoryWalker::new();
        walker.expect_walk()
            .return_once(move |root, _| Ok(vec![
                create_entry(root, "Cargo.lock", false),
                create_entry(root, "src/main.rs", false)
            ]));

        let mut binary = MockBinaryChecker::new();
        binary.expect_is_binary_or_image().returning(|_| Ok(false));

        let processor = DirectoryProcessor::with_deps(walker, binary);
        let result = processor.process(root, &config, false, false, false).unwrap();

        assert!(result.tree.contains("Cargo.lock"));
        assert!(!result.files_to_include.iter().any(|p| p.ends_with("Cargo.lock")));
        assert!(result.files_to_include.iter().any(|p| p.ends_with("main.rs")));
    }

    #[test]
    fn test_binary_files_completely_excluded() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        let config = Config::default();

        let mut walker = MockDirectoryWalker::new();
        walker.expect_walk()
            .return_once(move |root, _| Ok(vec![
                create_entry(root, "logo.png", false),
                create_entry(root, "README.md", false)
            ]));

        let mut binary = MockBinaryChecker::new();
        binary.expect_is_binary_or_image()
            .withf(|p| p.to_string_lossy().ends_with("logo.png"))
            .returning(|_| Ok(true));
        binary.expect_is_binary_or_image()
            .withf(|p| p.to_string_lossy().ends_with("README.md"))
            .returning(|_| Ok(false));

        let processor = DirectoryProcessor::with_deps(walker, binary);
        let result = processor.process(root, &config, false, false, false).unwrap();

        assert!(!result.tree.contains("logo.png"));
        assert!(!result.files_to_include.iter().any(|p| p.ends_with("logo.png")));
        assert!(result.tree.contains("README.md"));
    }

    #[test]
    fn test_skip_tests_injects_patterns() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        let config = Config::default();

        let mut walker = MockDirectoryWalker::new();
        walker.expect_walk()
            .return_once(move |root, _| Ok(vec![
                create_entry(root, "main_test.go", false),
                create_entry(root, "main.go", false)
            ]));

        let mut binary = MockBinaryChecker::new();
        binary.expect_is_binary_or_image().returning(|_| Ok(false));

        let processor = DirectoryProcessor::with_deps(walker, binary);
        let result = processor.process(root, &config, false, true, false).unwrap();

        assert!(result.tree.contains("main_test.go"));
        assert!(!result.files_to_include.iter().any(|p| p.ends_with("main_test.go")));
        assert!(result.files_to_include.iter().any(|p| p.ends_with("main.go")));
    }

    #[test]
    fn test_nested_config_integration() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        let sub = root.join("sub");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join(".r2t.yaml"), "ignore-content:\n  - secret.txt\n").unwrap();

        let config = Config::default();

        let mut walker = MockDirectoryWalker::new();
        walker.expect_walk()
            .return_once(move |root, _| Ok(vec![
                create_entry(root, "sub", true),
                create_entry(root, "sub/secret.txt", false),
                create_entry(root, "sub/visible.txt", false)
            ]));

        let mut binary = MockBinaryChecker::new();
        binary.expect_is_binary_or_image().returning(|_| Ok(false));

        let processor = DirectoryProcessor::with_deps(walker, binary);
        let result = processor.process(root, &config, false, false, false).unwrap();

        assert!(result.tree.contains("sub"));
        assert!(result.tree.contains("secret.txt"));
        assert!(!result.files_to_include.iter().any(|p| p.ends_with("secret.txt")));
        assert!(result.files_to_include.iter().any(|p| p.ends_with("visible.txt")));
    }
}