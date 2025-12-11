use clap::{Parser, ValueEnum};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(ValueEnum, Clone, Debug, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FormatArg {
    Yaml,
    Json,
    /// Used by the original repo-to-text
    Xml,
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// The root directory of the repository to process.
    #[arg(default_value = ".")]
    pub path: PathBuf,

    /// Directory to save the output file. Defaults to the input directory.
    #[arg(short, long)]
    pub output_dir: Option<PathBuf>,

    /// The output format for the final text file.
    #[arg(long, value_enum)]
    pub format: Option<FormatArg>,

    /// Output the result to stdout instead of a file.
    #[arg(long)]
    pub stdout: bool,

    /// Do not respect .gitignore files for filtering.
    #[arg(long)]
    pub no_gitignore: bool,

    /// Skip including the content of test files and inline test modules.
    #[arg(long)]
    pub skip_tests: bool,

    /// Disable merging of nested .r2t.yaml configuration files.
    #[arg(long)]
    pub no_merge: bool,

    /// Create a default .r2t.yaml settings file in the current directory.
    #[arg(long, conflicts_with_all = &["path", "output_dir", "stdout", "no_gitignore", "skip_tests", "format", "no_merge"])]
    pub create_settings: bool,

    /// Use with --create-settings to create a global configuration file.
    #[arg(long, requires = "create_settings")]
    pub global: bool,
}