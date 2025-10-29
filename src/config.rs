use crate::cli::FormatArg;
use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Default)]
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

    fn from_path(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file at {:?}", path))?;
        serde_yaml::from_str(&content)
            .with_context(|| format!("Failed to parse YAML from config file at {:?}", path))
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
# Defaults to yaml if not specified.
# format: yaml

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