use crate::config::Config;
use anyhow::{Context, Result};
use globset::{Error, Glob, GlobSet, GlobSetBuilder};
use ignore::{DirEntry as IgnoreDirEntry, WalkBuilder};
use ptree::{write_tree, TreeBuilder};
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct ProcessResult {
    pub tree: String,
    pub files_to_include: Vec<PathBuf>,
}
#[derive(Clone, Debug)]
pub struct DirectoryEntry {
    path: PathBuf,
    is_dir: bool,
    file_name: OsString,
}

impl DirectoryEntry {
    pub fn new(path: PathBuf, is_dir: bool) -> Self {
        let file_name = path.file_name().unwrap_or_default().to_owned();
        Self {
            path,
            is_dir,
            file_name,
        }
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

    pub fn file_name(&self) -> &OsString {
        &self.file_name
    }
}

impl From<IgnoreDirEntry> for DirectoryEntry {
    fn from(entry: IgnoreDirEntry) -> Self {
        let path = entry.path().to_path_buf();
        let is_dir = entry.file_type().map_or(false, |ft| ft.is_dir());
        Self::new(path, is_dir)
    }
}
pub trait DirectoryWalker {
    fn walk(&self, path: &Path, no_gitignore: bool) -> Result<Vec<DirectoryEntry>>;
}
pub trait BinaryChecker {
    fn is_binary_or_image(&self, path: &Path) -> Result<bool>;
}
pub struct IgnoreWalker;

impl DirectoryWalker for IgnoreWalker {
    fn walk(&self, path: &Path, no_gitignore: bool) -> Result<Vec<DirectoryEntry>> {
        let walker = WalkBuilder::new(path)
            .hidden(false)
            .git_ignore(!no_gitignore)
            .require_git(false)
            .git_global(false)
            .git_exclude(false)
            .sort_by_file_path(|a, b| a.cmp(b))
            .build();

        let mut entries = Vec::new();
        for result in walker {
            let entry = result.with_context(|| "Failed to process directory entry")?;
            entries.push(entry.into());
        }
        Ok(entries)
    }
}

pub struct FileBinaryChecker;

impl BinaryChecker for FileBinaryChecker {
    fn is_binary_or_image(&self, path: &Path) -> Result<bool> {
        crate::files::is_binary_or_image(path)
    }
}

pub struct DirectoryProcessor {
    walker: Box<dyn DirectoryWalker>,
    binary_checker: Box<dyn BinaryChecker>,
}

impl DirectoryProcessor {
    /// Create a new processor with production dependencies
    pub fn new() -> Self {
        Self {
            walker: Box::new(IgnoreWalker),
            binary_checker: Box::new(FileBinaryChecker),
        }
    }

    /// Create a processor with custom dependencies (for testing)
    #[cfg(test)]
    pub fn with_deps(
        walker: Box<dyn DirectoryWalker>,
        binary_checker: Box<dyn BinaryChecker>,
    ) -> Self {
        Self {
            walker,
            binary_checker,
        }
    }

    /// Process a directory and return the tree and files to include
    pub fn process(&self, path: &Path, config: &Config, no_gitignore: bool) -> Result<ProcessResult> {
        let mut files_to_include = Vec::new();
        let mut tree_nodes: HashMap<PathBuf, Vec<DirectoryEntry>> = HashMap::new();
        
        let mut all_content_only_patterns = config.ignore_content.clone();
        all_content_only_patterns.push(".r2t.yaml".to_string());
        all_content_only_patterns.push(".gitignore".to_string());
        
        let mut all_tree_and_content_patterns = config.ignore_tree_and_content.clone();
        all_tree_and_content_patterns.push(".git/".to_string());

        let tree_and_content_ignores = build_glob_set(&all_tree_and_content_patterns)?;
        let content_only_ignores = build_glob_set(&all_content_only_patterns)?;

        let entries = self.walker.walk(path, no_gitignore)?;

        for entry in entries {
            if entry.path() == path {
                continue;
            }

            let relative_path = entry.path().strip_prefix(path)?;
            let is_dir = entry.is_dir();
            
            if matches_ignore_pattern(relative_path, &tree_and_content_ignores, is_dir) {
                continue;
            }

            if self.binary_checker.is_binary_or_image(entry.path())? {
                continue;
            }

            if let Some(parent) = entry.path().parent() {
                tree_nodes
                    .entry(parent.to_path_buf())
                    .or_default()
                    .push(entry.clone());
            }

            if entry.is_file() {
                if !matches_ignore_pattern(relative_path, &content_only_ignores, false) {
                    files_to_include.push(entry.path().to_path_buf());
                }
            }
        }

        let mut tree_builder = TreeBuilder::new(
            path.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
        );
        build_ptree_recursive(path, &tree_nodes, &mut tree_builder);

        let tree_item = tree_builder.build();
        let mut buffer = Vec::new();
        write_tree(&tree_item, &mut buffer)?;
        let tree = String::from_utf8(buffer)?;

        Ok(ProcessResult {
            tree,
            files_to_include,
        })
    }
}

impl Default for DirectoryProcessor {
    fn default() -> Self {
        Self::new()
    }
}

fn build_glob_set(patterns: &[String]) -> std::result::Result<GlobSet, Error> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(Glob::new(pattern)?);
        
        if pattern.ends_with('/') {
            let dir_pattern = pattern.trim_end_matches('/');
            builder.add(Glob::new(dir_pattern)?);
            let recursive_pattern = format!("{}**", pattern);
            builder.add(Glob::new(&recursive_pattern)?);
        }
    }
    builder.build()
}

fn matches_ignore_pattern(relative_path: &Path, glob_set: &GlobSet, is_dir: bool) -> bool {
    if glob_set.is_match(relative_path) {
        return true;
    }

    if is_dir {
        let path_with_slash = format!("{}/", relative_path.to_string_lossy());
        if glob_set.is_match(path_with_slash.as_str()) {
            return true;
        }
    }

    false
}

fn build_ptree_recursive(
    current_path: &Path,
    nodes: &HashMap<PathBuf, Vec<DirectoryEntry>>,
    builder: &mut TreeBuilder,
) {
    if let Some(children) = nodes.get(current_path) {
        for child in children {
            let child_name = child.file_name().to_string_lossy().into_owned();
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

    fn create_test_config(
        ignore_tree_and_content: Vec<String>,
        ignore_content: Vec<String>,
    ) -> Config {
        Config {
            ignore_tree_and_content,
            ignore_content,
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

        let path = Path::new("debug.log");
        assert!(matches_ignore_pattern(path, &glob_set, false));

        let path = Path::new("temp.txt");
        assert!(matches_ignore_pattern(path, &glob_set, false));

        let path = Path::new("main.rs");
        assert!(!matches_ignore_pattern(path, &glob_set, false));
    }

    #[test]
    fn test_matches_ignore_pattern_directory() {
        let patterns = vec!["node_modules/".to_string()];
        let glob_set = build_glob_set(&patterns).unwrap();

        let path = Path::new("node_modules");
        assert!(matches_ignore_pattern(path, &glob_set, true));

        let path = Path::new("node_modules/package");
        assert!(matches_ignore_pattern(path, &glob_set, false));

        let path = Path::new("src");
        assert!(!matches_ignore_pattern(path, &glob_set, true));
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

    // Unit tests using mocks - NO filesystem interaction
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

        let processor = DirectoryProcessor::with_deps(
            Box::new(mock_walker),
            Box::new(mock_binary),
        );

        let config = create_test_config(
            vec!["*.log".to_string(), "node_modules/".to_string()],
            vec![],
        );
        let result = processor.process(&base_path, &config, false)?;

        assert!(!result
            .files_to_include
            .iter()
            .any(|p| p.ends_with("exclude.log")));
        assert!(!result.tree.contains("exclude.log"));

        assert!(!result.tree.contains("node_modules"));

        assert!(result
            .files_to_include
            .iter()
            .any(|p| p.ends_with("include.txt")));

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

        let processor = DirectoryProcessor::with_deps(
            Box::new(mock_walker),
            Box::new(mock_binary),
        );

        let config = create_test_config(vec![], vec!["hidden.txt".to_string()]);
        let result = processor.process(&base_path, &config, false)?;

        assert!(result.tree.contains("hidden.txt"));
        assert!(!result
            .files_to_include
            .iter()
            .any(|p| p.ends_with("hidden.txt")));

        assert!(result.tree.contains("visible.txt"));
        assert!(result
            .files_to_include
            .iter()
            .any(|p| p.ends_with("visible.txt")));

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

        let processor = DirectoryProcessor::with_deps(
            Box::new(mock_walker),
            Box::new(mock_binary),
        );

        let config = create_test_config(vec![], vec![]);
        let result = processor.process(&base_path, &config, false)?;

        assert!(result.tree.contains(".r2t.yaml"));
        assert!(!result
            .files_to_include
            .iter()
            .any(|p| p.ends_with(".r2t.yaml")));

        assert!(result
            .files_to_include
            .iter()
            .any(|p| p.ends_with("other.txt")));

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

        let mut mock_binary = MockBinaryChecker::new();
        mock_binary
            .expect_is_binary_or_image()
            .returning(|path| Ok(path.extension().and_then(|s| s.to_str()) == Some("png")));

        let processor = DirectoryProcessor::with_deps(
            Box::new(mock_walker),
            Box::new(mock_binary),
        );

        let config = create_test_config(vec![], vec![]);
        let result = processor.process(&base_path, &config, false)?;

        assert!(!result.tree.contains("image.png"));
        assert!(!result
            .files_to_include
            .iter()
            .any(|p| p.ends_with("image.png")));

        assert!(result.tree.contains("text.txt"));
        assert!(result
            .files_to_include
            .iter()
            .any(|p| p.ends_with("text.txt")));

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

        let processor = DirectoryProcessor::with_deps(
            Box::new(mock_walker),
            Box::new(mock_binary),
        );

        let config = create_test_config(vec![], vec![]);
        let result = processor.process(&base_path, &config, false)?;

        assert!(result.tree.contains("src"));
        assert!(result.tree.contains("models"));
        assert!(result.tree.contains("controllers"));
        assert!(result.tree.contains("main.rs"));
        assert!(result.tree.contains("user.rs"));
        assert!(result.tree.contains("api.rs"));

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

        let processor = DirectoryProcessor::with_deps(
            Box::new(mock_walker),
            Box::new(mock_binary),
        );

        let config = create_test_config(vec![], vec![]);
        let result = processor.process(&base_path, &config, false)?;

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

        let processor = DirectoryProcessor::with_deps(
            Box::new(mock_walker),
            Box::new(mock_binary),
        );

        let config = create_test_config(
            vec!["hide_completely.txt".to_string()],
            vec!["show_in_tree_only.txt".to_string()],
        );
        let result = processor.process(&base_path, &config, false)?;

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

        let processor = DirectoryProcessor::with_deps(
            Box::new(mock_walker),
            Box::new(mock_binary),
        );

        let config = create_test_config(vec![], vec![]);
        let result = processor.process(&base_path, &config, false);

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Walker error"));
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

        let processor = DirectoryProcessor::with_deps(
            Box::new(mock_walker),
            Box::new(mock_binary),
        );

        let config = create_test_config(vec![], vec![]);
        let result = processor.process(&base_path, &config, false);

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Binary check error"));
    }
}