use crate::config::Config;
use anyhow::{Context, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// compiled configuration for a specific path scope
#[derive(Debug, Clone)]
pub struct PathConfig {
    pub tree_and_content_ignores: GlobSet,
    pub content_only_ignores: GlobSet,
    /// We keep the raw config to use as a base for subdirectories
    pub raw_config: Arc<Config>,
}

pub struct ConfigResolver {
    root_path: PathBuf,
    /// Cache of directory path -> Effective Configuration
    cache: HashMap<PathBuf, Arc<PathConfig>>,
    no_merge: bool,
}

impl ConfigResolver {
    pub fn new(root_config: Config, root_path: PathBuf, no_merge: bool) -> Result<Self> {
        let mut resolver = Self {
            root_path,
            cache: HashMap::new(),
            no_merge,
        };

        let root_path_config = resolver.compile_config(root_config)?;
        resolver.cache.insert(resolver.root_path.clone(), Arc::new(root_path_config));

        Ok(resolver)
    }

    /// Returns the effective configuration for a specific directory or file path.
    pub fn get_config(&mut self, path: &Path) -> Result<Arc<PathConfig>> {
        let dir = if path.is_file() {
            path.parent().unwrap_or(path)
        } else {
            path
        };

        if let Some(config) = self.cache.get(dir) {
            return Ok(config.clone());
        }

        self.resolve_recursive(dir)
    }

    fn resolve_recursive(&mut self, dir: &Path) -> Result<Arc<PathConfig>> {
        if let Some(config) = self.cache.get(dir) {
            return Ok(config.clone());
        }

        if !dir.starts_with(&self.root_path) || dir == self.root_path {
            return Ok(self.cache.get(&self.root_path).unwrap().clone());
        }
        let parent = dir.parent().unwrap_or(dir);
        let parent_config = self.resolve_recursive(parent)?;

        if self.no_merge {
            self.cache.insert(dir.to_path_buf(), parent_config.clone());
            return Ok(parent_config);
        }

        let local_config_path = dir.join(".r2t.yaml");
        if local_config_path.exists() {
            let loaded_config = Config::load_from_file(&local_config_path)
                .unwrap_or_else(|_| Config::default());

            let prefix = dir.strip_prefix(&self.root_path).unwrap_or(Path::new(""));

            let mut new_raw_config = (*parent_config.raw_config).clone();
            new_raw_config.merge(loaded_config, prefix);

            let new_path_config = self.compile_config(new_raw_config)?;
            let arc_config = Arc::new(new_path_config);

            self.cache.insert(dir.to_path_buf(), arc_config.clone());
            Ok(arc_config)
        } else {
            self.cache.insert(dir.to_path_buf(), parent_config.clone());
            Ok(parent_config)
        }
    }

    fn compile_config(&self, config: Config) -> Result<PathConfig> {
        Ok(PathConfig {
            tree_and_content_ignores: build_glob_set(&config.ignore_tree_and_content)
                .context("Failed to build tree ignores")?,
            content_only_ignores: build_glob_set(&config.ignore_content)
                .context("Failed to build content ignores")?,
            raw_config: Arc::new(config),
        })
    }
}

/// Builds a GlobSet from a list of patterns.
/// Copied logic to ensure Resolver is self-contained.
fn build_glob_set(patterns: &[String]) -> Result<GlobSet, globset::Error> {
    let mut builder = GlobSetBuilder::new();

    for pattern in patterns {
        builder.add(Glob::new(pattern)?);

        // For directory patterns ending in /, add recursive matching
        if let Some(dir_pattern) = pattern.strip_suffix('/') {
            builder.add(Glob::new(dir_pattern)?);
            builder.add(Glob::new(&format!("{}/**", dir_pattern))?);
        }
    }

    builder.build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_resolver(root: &Path, no_merge: bool) -> ConfigResolver {
        let config = Config::load(root).expect("load root config");
        ConfigResolver::new(config, root.to_path_buf(), no_merge).expect("create resolver")
    }

    fn write_config(dir: &Path, patterns: &[&str]) {
        let content = patterns
            .iter()
            .fold("ignore-tree-and-content:\n".to_string(), |acc, p| {
                format!("{}  - \"{}\"\n", acc, p)
            });
        fs::write(dir.join(".r2t.yaml"), content).expect("write config");
    }

    fn matches(resolver: &mut ConfigResolver, path: &Path) -> bool {
        let config = resolver.get_config(path).expect("get config");
        let relative = path.strip_prefix(&resolver.root_path).expect("strip prefix");
        let is_dir = path.extension().is_none();

        config.tree_and_content_ignores.is_match(relative)
            || (is_dir && config.tree_and_content_ignores.is_match(format!("{}/", relative.display())))
    }

    #[test]
    fn root_patterns_apply_to_deeply_nested_paths() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        write_config(root, &["**/*.log"]);
        fs::create_dir_all(root.join("src/utils")).unwrap();

        let mut resolver = setup_resolver(root, false);

        assert!(matches(&mut resolver, &root.join("src/utils/debug.log")));
        assert!(!matches(&mut resolver, &root.join("src/utils/info.txt")));
    }

    #[test]
    fn nested_configs_merge_with_parent() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        let frontend = root.join("frontend");

        write_config(root, &["target/"]);
        fs::create_dir(&frontend).unwrap();
        write_config(&frontend, &["node_modules/"]);

        let mut resolver = setup_resolver(root, false);
        let config = resolver.get_config(&frontend.join("src/index.ts")).unwrap();

        assert!(config.tree_and_content_ignores.is_match("target"));
        assert!(config.tree_and_content_ignores.is_match("frontend/node_modules"));
    }

    #[test]
    fn nested_config_patterns_are_scoped_to_their_directory() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        let frontend = root.join("frontend");
        let backend = root.join("backend");

        fs::create_dir(&frontend).unwrap();
        fs::create_dir(&backend).unwrap();
        write_config(&frontend, &["*.css"]);

        let mut resolver = setup_resolver(root, false);

        assert!(matches(&mut resolver, &frontend.join("style.css")));
        assert!(!matches(&mut resolver, &backend.join("style.css")));
    }

    #[test]
    fn no_merge_flag_ignores_nested_configs() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        let subdir = root.join("subdir");

        write_config(root, &["root.txt"]);
        fs::create_dir(&subdir).unwrap();
        write_config(&subdir, &["sub.txt"]);

        let mut resolver = setup_resolver(root, true);

        assert!(!matches(&mut resolver, &subdir.join("sub.txt")));
    }

    #[test]
    fn directory_patterns_match_only_at_defined_level() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        let dir_a = root.join("a");
        let dir_b = dir_a.join("b");

        fs::create_dir(&dir_a).unwrap();
        write_config(&dir_a, &["tmp/"]);
        fs::create_dir(&dir_b).unwrap();
        write_config(&dir_b, &["cache/"]);

        let mut resolver = setup_resolver(root, false);

        assert!(matches(&mut resolver, &dir_a.join("tmp/file")));
        assert!(matches(&mut resolver, &dir_b.join("cache/file")));
        assert!(!matches(&mut resolver, &dir_b.join("tmp/file")));
    }

    #[test]
    fn paths_outside_root_use_root_config() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        write_config(root, &["foo"]);

        let mut resolver = setup_resolver(root, false);
        let outside = if cfg!(windows) {
            PathBuf::from("C:\\outside")
        } else {
            PathBuf::from("/outside")
        };

        let config = resolver.get_config(&outside).expect("get config for outside path");
        assert!(config.tree_and_content_ignores.is_match("foo"));
    }
}