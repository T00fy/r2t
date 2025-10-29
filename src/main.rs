use anyhow::{Context, Result};
use clap::Parser;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

mod cli;
mod config;
mod files;
mod output;
mod processor;
mod stripper;

use crate::cli::{Cli, FormatArg};
use crate::config::Config;
use crate::output::OutputFormat;
use crate::processor::DirectoryProcessor;

fn determine_format(cli_format: Option<FormatArg>, config_format: Option<FormatArg>) -> OutputFormat {
    match cli_format.or(config_format) {
        Some(FormatArg::Json) => OutputFormat::Json,
        Some(FormatArg::Xml) => OutputFormat::Xml,
        _ => OutputFormat::Yaml,
    }
}

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
    let output_format = determine_format(cli.format, config.format);

    let project_name = start_path
        .canonicalize()?
        .file_name()
        .and_then(|n| n.to_str())
        .context("Failed to extract directory name from path")?
        .to_owned();

    let process_result = DirectoryProcessor::new().process(
        &start_path,
        &config,
        cli.no_gitignore,
        cli.skip_tests,
    )?;

    let mut contents = Vec::new();
    for file_path in &process_result.files_to_include {
        let relative_path = file_path.strip_prefix(&start_path).with_context(|| {
            format!(
                "Failed to strip prefix from '{}'",
                file_path.to_string_lossy()
            )
        })?;

        let raw_content = files::read_file_contents(file_path)
            .with_context(|| format!("Failed to read file: {}", file_path.display()))?;

        let content = if cli.skip_tests {
            stripper::strip_inline_tests(file_path, &raw_content)
        } else {
            raw_content
        };

        contents.push(output::FileContent {
            full_path: relative_path.to_string_lossy().into_owned(),
            content,
        });
    }

    let repo_representation = output::RepoRepresentation {
        directory: project_name,
        directory_structure: process_result.tree,
        contents,
    };

    let final_output = output::render(output_format, &repo_representation)?;

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

        fs::write(&output_path, &final_output)
            .with_context(|| format!("Failed to write output file: {:?}", output_path))?;

        println!(
            "[SUCCESS] Repository structure and contents successfully saved to file: \"{}\"",
            output_path.to_string_lossy()
        );
    }

    Ok(())
}