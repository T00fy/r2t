use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// The root directory of the repository to process.
    #[arg(default_value = ".")]
    pub path: PathBuf,

    /// Directory to save the output file. Defaults to the input directory.
    #[arg(short, long)]
    pub output_dir: Option<PathBuf>,

    /// Output the result to stdout instead of a file.
    #[arg(long)]
    pub stdout: bool,

    /// Do not respect .gitignore files for filtering.
    #[arg(long)]
    pub no_gitignore: bool,

    /// Create a default .r2t.yaml settings file in the current directory.
    #[arg(long, conflicts_with_all = &["path", "output_dir", "stdout", "no_gitignore"])]
    pub create_settings: bool,

    /// Use with --create-settings to create a global configuration file.
    #[arg(long, requires = "create_settings")]
    pub global: bool,
}