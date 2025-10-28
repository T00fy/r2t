use anyhow::{Context, Result};
use clap::Parser;
use std::fs;
use std::io::Write;
use std::path::{PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

mod cli;
mod config;
mod files;
mod output;
mod processor;
mod stripper;

use cli::Cli;
use config::Config;
use crate::processor::DirectoryProcessor;

fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.create_settings {
        let path = if cli.global {
            Config::get_global_path()?
        } else {
            PathBuf::from(".r2t.yaml")
        };
        Config::create_default_config(&path)?;
        println!(
            "Default settings file created at: {}",
            path.to_string_lossy()
        );
        return Ok(());
    }

    let start_path = cli.path;
    let config = Config::load(&start_path)?;

    let project_name = start_path
        .canonicalize()?
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let process_result = DirectoryProcessor::new().process(
        &start_path,
        &config,
        cli.no_gitignore,
        cli.skip_tests,
    )?;

    let final_output = output::generate_output(
        &project_name,
        &process_result.tree,
        &process_result.files_to_include,
        &start_path,
        cli.skip_tests
    )?;

    if cli.stdout {
        print!("{}", final_output);
    } else {
        let output_dir = cli.output_dir.unwrap_or_else(|| start_path.clone());
        if !output_dir.exists() {
            fs::create_dir_all(&output_dir)
                .with_context(|| format!("Failed to create output directory: {:?}", output_dir))?;
        }

        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let filename = format!("repo-to-text_{}.txt", timestamp);
        let output_path = output_dir.join(filename);

        let mut file = fs::File::create(&output_path)
            .with_context(|| format!("Failed to create output file: {:?}", output_path))?;
        file.write_all(final_output.as_bytes())
            .with_context(|| format!("Failed to write to output file: {:?}", output_path))?;

        println!(
            "[SUCCESS] Repository structure and contents successfully saved to file: \"{}\"",
            output_path.to_string_lossy()
        );
    }

    Ok(())
}