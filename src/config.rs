use crate::cli::FormatArg;
use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct Config {
    #[serde(default)]
    pub format: Option<FormatArg>,
    #[serde(default)]
    pub ignore_tree_and_content: Vec<String>,
    #[serde(default)]
    pub ignore_content: Vec<String>,
}

impl Config {
    pub fn load(start_path: &Path) -> Result<Self> {
        let local_path = start_path.join(".r2t.yaml");
        if local_path.exists() {
            return Self::from_path(&local_path);
        }

        if let Some(global_path) = Self::get_global_path().ok().filter(|p| p.exists()) {
            return Self::from_path(&global_path);
        }

        Ok(Config::default())
    }

    pub fn load_from_file(path: &Path) -> Result<Self> {
        Self::from_path(path)
    }

    fn from_path(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file at {:?}", path))?;
        serde_yaml::from_str(&content)
            .with_context(|| format!("Failed to parse YAML from config file at {:?}", path))
    }

    /// Merges another config into this one.
    /// Patterns from the `other` config are prefixed with the given `prefix`
    /// so they apply correctly relative to the project root.
    pub fn merge(&mut self, other: Config, prefix: &Path) {
        // Helper to prefix a single glob pattern
        let prefix_pattern = |pattern: String| -> String {
            // If the pattern matches everything, or is empty, we probably don't need to prefix strictly,
            // but standard gitignore behavior suggests all patterns in a subdir are relative to it.
            // We use forward slashes for globs.
            let prefix_str = prefix.to_string_lossy().replace('\\', "/");

            if prefix_str.is_empty() {
                return pattern;
            }

            // Handle negation
            if let Some(stripped) = pattern.strip_prefix('!') {
                format!("!{}/{}", prefix_str, stripped)
            } else if pattern.starts_with('/') {
                // If pattern is anchored at root of the config file (e.g. /foo), 
                // it becomes anchored at the subdir (e.g. /subdir/foo).
                // globset treats patterns starting with / as anchored.
                // We strip the leading / from the pattern and append to prefix.
                format!("{}/{}", prefix_str, pattern.trim_start_matches('/'))
            } else {
                // Standard pattern (e.g. *.log or node_modules/)
                format!("{}/{}", prefix_str, pattern)
            }
        };

        // Extend ignore lists
        self.ignore_tree_and_content.extend(
            other.ignore_tree_and_content.into_iter().map(prefix_pattern)
        );
        self.ignore_content.extend(
            other.ignore_content.into_iter().map(prefix_pattern)
        );

        // Note: We intentionally do not merge 'format'. 
        // The root config (or CLI arg) dictates the output format.
    }

    pub fn get_global_path() -> Result<PathBuf> {
        if let Some(proj_dirs) = ProjectDirs::from("com", "r2t", "r2t") {
            Ok(proj_dirs.config_dir().join("config.yaml"))
        } else {
            anyhow::bail!("Could not determine global config directory.")
        }
    }

    pub fn create_default_config(path: &Path) -> Result<()> {
        let default_settings = r#"# r2t settings file - https://github.com/T00fy/r2t
# Syntax: gitignore-style glob patterns

# The output format. Can be: yaml, json, xml
# Defaults to xml if not specified.
# format: xml

# Ignore files and directories for both the tree view and content sections.
ignore-tree-and-content:
  - ".git/"
  - "target/"
  - "node_modules/"
  - ".idea/"
  - "*.log"
  - ".terraform/"

# Ignore files only for the content section (they will still appear in the tree).
ignore-content:
  - "LICENSE"
  - "*.lock"
  - ".r2t.yaml"
"#;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, default_settings)?;
        Ok(())
    }
}